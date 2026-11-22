//! Windows HID transport. Only the 06 B0 status query is sent to the base station.

use std::{
	io::Error,
	mem::{size_of, zeroed},
	os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
	ptr::{null, null_mut},
	slice,
	time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use windows_sys::{
	Win32::{
		Devices::{
			DeviceAndDriverInstallation::{
				DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO, SP_DEVICE_INTERFACE_DATA,
				SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
				SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
			},
			HumanInterfaceDevice::{
				HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData, HidD_GetAttributes,
				HidD_GetHidGuid, HidD_GetPreparsedData, HidP_GetCaps, PHIDP_PREPARSED_DATA,
			},
		},
		Foundation::{
			ERROR_IO_PENDING, ERROR_NO_MORE_ITEMS, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE,
			WAIT_OBJECT_0, WAIT_TIMEOUT,
		},
		Storage::FileSystem::{
			CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
		},
		System::{
			IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
			Threading::{CreateEventW, WaitForSingleObject},
		},
	},
	core::GUID,
};

use crate::battery::{Battery, STATUS_HEADER};

const VENDOR_ID: u16 = 0x1038;
/// Nova Pro Wireless and Nova Pro Wireless X base stations.
const PRODUCT_IDS: [u16; 2] = [0x12e0, 0x12e5];
/// The USB interface that carries the status collection.
const INTERFACE: &str = "mi_04";
const STATUS_USAGE_PAGE: u16 = 0xffc0;
const REPORT_LEN: usize = 64;
const IO_TIMEOUT: Duration = Duration::from_millis(1500);

struct DeviceSet(HDEVINFO);

impl Drop for DeviceSet {
	fn drop(&mut self) {
		// SAFETY: this wrapper owns the set returned by SetupDiGetClassDevsW.
		unsafe { SetupDiDestroyDeviceInfoList(self.0) };
	}
}

struct Preparsed(PHIDP_PREPARSED_DATA);

impl Drop for Preparsed {
	fn drop(&mut self) {
		// SAFETY: this wrapper owns the allocation returned by HidD_GetPreparsedData.
		unsafe { HidD_FreePreparsedData(self.0) };
	}
}

fn open(path: &[u16], access: u32, flags: u32) -> Result<OwnedHandle> {
	// SAFETY: callers supply a null-terminated UTF-16 path from SetupAPI. No security
	// attributes or template are used, and access is shared with other device clients.
	let handle = unsafe {
		CreateFileW(path.as_ptr(), access, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, flags, null_mut())
	};
	ensure!(handle != INVALID_HANDLE_VALUE, "Could not open the base station: {}", Error::last_os_error());
	// SAFETY: CreateFileW succeeded and ownership is transferred exactly once.
	Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn is_status_interface(path: &[u16]) -> Result<bool> {
	let handle = open(path, 0, 0)?;
	// SAFETY: these are plain Windows output structures whose fields accept zero initialization.
	let mut attributes: HIDD_ATTRIBUTES = unsafe { zeroed() };
	attributes.Size = u32::try_from(size_of::<HIDD_ATTRIBUTES>())?;
	// SAFETY: the handle and output buffer remain valid for the duration of the call.
	if !unsafe { HidD_GetAttributes(handle.as_raw_handle(), &raw mut attributes) } {
		return Err(Error::last_os_error()).context("Could not read HID device attributes");
	}
	if attributes.VendorID != VENDOR_ID || !PRODUCT_IDS.contains(&attributes.ProductID) {
		return Ok(false);
	}
	let mut data = 0;
	// SAFETY: the handle is open, and data points to writable storage.
	if !unsafe { HidD_GetPreparsedData(handle.as_raw_handle(), &raw mut data) } {
		return Err(Error::last_os_error()).context("Could not read HID capabilities");
	}
	let data = Preparsed(data);
	// SAFETY: HIDP_CAPS is a plain Windows output structure.
	let mut caps: HIDP_CAPS = unsafe { zeroed() };
	// SAFETY: the preparsed data allocation and output buffer are alive during this call.
	let result = unsafe { HidP_GetCaps(data.0, &raw mut caps) };
	ensure!(result == HIDP_STATUS_SUCCESS, "Could not decode HID capabilities");
	let report_len = u16::try_from(REPORT_LEN)?;
	Ok(caps.UsagePage == STATUS_USAGE_PAGE
		&& caps.Usage == 1
		&& caps.InputReportByteLength == report_len
		&& caps.OutputReportByteLength == report_len)
}

fn interface_path(set: &DeviceSet, interface: &SP_DEVICE_INTERFACE_DATA) -> Result<Vec<u16>> {
	let mut needed = 0;
	// SAFETY: this first call only asks for the required buffer size.
	unsafe { SetupDiGetDeviceInterfaceDetailW(set.0, interface, null_mut(), 0, &raw mut needed, null_mut()) };
	ensure!(needed >= 6, "Could not size the HID interface path");
	// usize storage provides sufficient alignment for SP_DEVICE_INTERFACE_DETAIL_DATA_W.
	let mut storage = vec![0usize; usize::try_from(needed)?.div_ceil(size_of::<usize>())];
	let detail = storage.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
	// SAFETY: storage is aligned and holds at least needed bytes. The Windows structure's
	// fixed header is followed by a variable-length UTF-16 device path at offset four.
	unsafe { (*detail).cbSize = u32::try_from(size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>())? };
	// SAFETY: all pointers refer to valid input structures or the sized, aligned output buffer.
	if unsafe { SetupDiGetDeviceInterfaceDetailW(set.0, interface, detail, needed, null_mut(), null_mut()) } == 0 {
		return Err(Error::last_os_error()).context("Could not read the HID interface path");
	}
	// SAFETY: the path occupies the allocated buffer after the four-byte header. We
	// limit the slice to that allocation and locate its terminator before copying it.
	let path = unsafe { slice::from_raw_parts((*detail).DevicePath.as_ptr(), usize::try_from(needed - 4)? / 2) };
	let end = path.iter().position(|&unit| unit == 0).context("Unterminated HID interface path")?;
	Ok(path[..=end].to_vec())
}

fn is_candidate_path(lowercase_path: &str) -> bool {
	PRODUCT_IDS.iter().any(|pid| lowercase_path.contains(&format!("vid_{VENDOR_ID:04x}&pid_{pid:04x}&{INTERFACE}")))
}

fn find_base_station() -> Result<OwnedHandle> {
	let mut guid = GUID::default();
	// SAFETY: guid points to writable storage for the HID interface class GUID.
	unsafe { HidD_GetHidGuid(&raw mut guid) };
	// SAFETY: use the HID class, present interfaces only, and no parent window/enumerator.
	let raw_set =
		unsafe { SetupDiGetClassDevsW(&raw const guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) };
	ensure!(raw_set != -1, "Could not enumerate HID interfaces: {}", Error::last_os_error());
	let set = DeviceSet(raw_set);
	let mut last_error = None;
	for index in 0.. {
		// SAFETY: SP_DEVICE_INTERFACE_DATA is an output structure with no owning pointers.
		let mut interface: SP_DEVICE_INTERFACE_DATA = unsafe { zeroed() };
		interface.cbSize = u32::try_from(size_of::<SP_DEVICE_INTERFACE_DATA>())?;
		// SAFETY: set and guid remain valid throughout enumeration, and interface is writable.
		if unsafe { SetupDiEnumDeviceInterfaces(set.0, null(), &raw const guid, index, &raw mut interface) } == 0 {
			// SAFETY: immediately reads the error from the failed enumeration call.
			if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
				break;
			}
			return Err(Error::last_os_error()).context("Could not enumerate HID interfaces");
		}
		let path = interface_path(&set, &interface)?;
		let name = String::from_utf16_lossy(&path).to_ascii_lowercase();
		// Only probe the known base stations' status interface.
		if !is_candidate_path(&name) {
			continue;
		}
		match is_status_interface(&path) {
			Ok(true) => return open(&path, GENERIC_READ | GENERIC_WRITE, FILE_FLAG_OVERLAPPED),
			Ok(false) => (),
			Err(error) => last_error = Some(error),
		}
	}
	if let Some(error) = last_error {
		return Err(error);
	}
	bail!("Arctis Nova Pro Wireless base station is not connected")
}

fn transfer(handle: &OwnedHandle, buffer: &mut [u8; REPORT_LEN], write: bool, timeout: Duration) -> Result<usize> {
	// SAFETY: create a private manual-reset event with no name or security attributes.
	let event = unsafe { CreateEventW(null(), 1, 0, null()) };
	ensure!(!event.is_null(), "Could not create a HID I/O event: {}", Error::last_os_error());
	// SAFETY: event is a newly created handle transferred to one owner.
	let event = unsafe { OwnedHandle::from_raw_handle(event) };
	// SAFETY: OVERLAPPED accepts zero initialization; hEvent is kept alive until completion.
	let mut operation = OVERLAPPED { hEvent: event.as_raw_handle(), ..unsafe { zeroed() } };
	let length = u32::try_from(REPORT_LEN)?;
	let mut count = 0;
	// SAFETY: handle was opened for overlapped I/O, buffer and operation remain alive
	// until completion (including cancellation), and count is a writable DWORD.
	let success = unsafe {
		if write {
			WriteFile(handle.as_raw_handle(), buffer.as_ptr(), length, &raw mut count, &raw mut operation)
		} else {
			ReadFile(handle.as_raw_handle(), buffer.as_mut_ptr(), length, &raw mut count, &raw mut operation)
		}
	};
	if success == 0 {
		// SAFETY: immediately reads the error from WriteFile/ReadFile.
		if unsafe { GetLastError() } != ERROR_IO_PENDING {
			return Err(Error::last_os_error()).context("HID transfer failed");
		}
		let milliseconds = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
		// SAFETY: event remains valid and is private to this operation.
		let wait = unsafe { WaitForSingleObject(event.as_raw_handle(), milliseconds) };
		if wait != WAIT_OBJECT_0 {
			let error = Error::last_os_error();
			// SAFETY: cancel only this operation, then wait for its terminal completion.
			// This prevents Windows from accessing stack/buffer memory after we return.
			unsafe {
				CancelIoEx(handle.as_raw_handle(), &raw const operation);
				GetOverlappedResult(handle.as_raw_handle(), &raw const operation, &raw mut count, 1);
			}
			if wait == WAIT_TIMEOUT {
				bail!("Base station status request timed out");
			}
			return Err(error).context("Could not wait for the HID transfer");
		}
		// SAFETY: the event signaled completion; all I/O storage remains alive.
		if unsafe { GetOverlappedResult(handle.as_raw_handle(), &raw const operation, &raw mut count, 0) } == 0 {
			return Err(Error::last_os_error()).context("HID transfer failed");
		}
	}
	Ok(usize::try_from(count)?)
}

pub fn read_battery() -> Result<Battery> {
	let handle = find_base_station()?;
	let mut request = [0; REPORT_LEN];
	request[..2].copy_from_slice(&STATUS_HEADER);
	ensure!(transfer(&handle, &mut request, true, IO_TIMEOUT)? == REPORT_LEN, "Incomplete HID status request");
	let deadline = Instant::now() + IO_TIMEOUT;
	loop {
		let remaining = deadline.saturating_duration_since(Instant::now());
		ensure!(!remaining.is_zero(), "Base station did not return a matching status report");
		let mut report = [0; REPORT_LEN];
		let length = transfer(&handle, &mut report, false, remaining)?;
		ensure!(length <= report.len(), "Invalid HID report length");
		if report[..length].starts_with(&STATUS_HEADER) {
			return Battery::from_report(&report[..length]);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::{is_candidate_path, read_battery};

	#[test]
	fn only_the_status_interface_of_known_stations_is_probed() {
		assert!(is_candidate_path(r"\\?\hid#vid_1038&pid_12e0&mi_04#7&1a2b&0&0000#{4d1e55b2}"));
		assert!(is_candidate_path(r"\\?\hid#vid_1038&pid_12e5&mi_04#7&1a2b&0&0000#{4d1e55b2}"));
		assert!(!is_candidate_path(r"\\?\hid#vid_1038&pid_12e0&mi_03#7&1a2b&0&0000#{4d1e55b2}"));
		assert!(!is_candidate_path(r"\\?\hid#vid_1038&pid_1234&mi_04#7&1a2b&0&0000#{4d1e55b2}"));
	}

	#[test]
	#[ignore = "Requires a connected Nova Pro Wireless base station"]
	fn live_hid_status() {
		println!("{}", read_battery().expect("Direct HID battery query failed"));
	}
}
