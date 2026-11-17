//! Pageant WM_COPYDATA transport with a current-user-only shared mapping.
use std::{
    ptr,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    System::{DataExchange::COPYDATASTRUCT, Memory::*, Threading::*},
    UI::WindowsAndMessaging::*,
};
const MAPPING_SIZE: usize = 8192;
static REQUEST: AtomicU64 = AtomicU64::new(1);
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Allocation(*mut core::ffi::c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct View(MEMORY_MAPPED_VIEW_ADDRESS);
impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.0);
        }
    }
}
fn error() -> String {
    std::io::Error::last_os_error().to_string()
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// A protected DACL granting only the current user full access; with `inherit`
/// its entry also passes to files and folders created inside (OICI).
fn current_user_security(inherit: bool) -> Result<Allocation, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(error());
    }
    let token = Handle(token);
    let mut size = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut size);
    }
    if size == 0 || size > 64 * 1024 {
        return Err("Invalid current-user token size.".into());
    }
    let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            data.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(error());
    }
    let user = unsafe { &*data.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err(error());
    }
    let sid_allocation = Allocation(sid.cast());
    let mut length = 0;
    while unsafe { *sid.add(length) } != 0 {
        length += 1;
    }
    let sid_text = String::from_utf16(unsafe { std::slice::from_raw_parts(sid, length) })
        .map_err(|_| "Invalid current-user SID.")?;
    drop(sid_allocation);
    let flags = if inherit { "OICI" } else { "" };
    let sddl = wide(&format!("D:P(A;{flags};GA;;;{sid_text})"));
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(error());
    }
    Ok(Allocation(descriptor))
}

/// Restrict an already-open key file before any private bytes are written.
pub(crate) fn restrict_key_file(file: &std::fs::File) -> Result<(), String> {
    restrict(file, false)
}

/// Restrict an open folder (WRITE_DAC) to the current user, including the
/// files and folders later created inside it, such as an editor's saves.
pub(crate) fn restrict_folder(folder: &std::fs::File) -> Result<(), String> {
    restrict(folder, true)
}

fn restrict(file: &std::fs::File, inherit: bool) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    let descriptor = current_user_security(inherit)?;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
        == 0
        || present == 0
        || dacl.is_null()
    {
        return Err("Cannot construct private-key permissions.".into());
    }
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
    }
    Ok(())
}

pub(super) fn exchange_cancellable(
    body: &[u8],
    timeout: Duration,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<Vec<u8>, String> {
    if body.len() + 4 > MAPPING_SIZE {
        return Err("Pageant request exceeds the shared buffer.".into());
    }
    let request = body.to_vec();
    wait_cancellable(move || exchange(&request, timeout), stop, timeout)
}

// WM_COPYDATA cannot be withdrawn once delivered. Keep its mapping owned by a
// bounded worker until SendMessageTimeout returns; cancellation abandons only
// the result, never frees memory while Pageant may still be accessing it.
fn wait_cancellable<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
    stop: &std::sync::atomic::AtomicBool,
    timeout: Duration,
) -> Result<T, String> {
    use std::sync::{atomic::AtomicUsize, mpsc};
    static ACTIVE: AtomicUsize = AtomicUsize::new(0);
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            ACTIVE.fetch_sub(1, Ordering::AcqRel);
        }
    }
    let deadline = std::time::Instant::now() + timeout;
    super::check_request(stop, deadline)?;
    ACTIVE
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < 8).then_some(count + 1)
        })
        .map_err(|_| "Too many pending Pageant requests; finish the existing agent prompts.")?;
    let slot = Slot;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("ssh4-pageant".into())
        .spawn(move || {
            let _slot = slot;
            let _ = sender.send(operation());
        })
        .map_err(|error| error.to_string())?;
    loop {
        super::check_request(stop, deadline)?;
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => {
                super::check_request(stop, deadline)?;
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("Pageant request worker stopped.".into()),
        }
    }
}

pub(super) fn exchange(body: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
    if body.len() + 4 > MAPPING_SIZE {
        return Err("Pageant request exceeds the shared buffer.".into());
    }
    let pageant = wide("Pageant");
    let window = unsafe { FindWindowW(pageant.as_ptr(), pageant.as_ptr()) };
    if window.is_null() {
        return Err("Pageant is not running.".into());
    }
    exchange_window(body, timeout, window)
}

fn exchange_window(body: &[u8], timeout: Duration, window: HWND) -> Result<Vec<u8>, String> {
    if body.len() + 4 > MAPPING_SIZE {
        return Err("Pageant request exceeds the shared buffer.".into());
    }
    let security = current_user_security(false)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let name = format!(
        "PageantRequest{:08x}.{:08x}.{:016x}",
        std::process::id(),
        unsafe { GetCurrentThreadId() },
        REQUEST.fetch_add(1, Ordering::Relaxed)
    );
    let wide_name = wide(&name);
    let handle = unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            &attributes,
            PAGE_READWRITE,
            0,
            MAPPING_SIZE as u32,
            wide_name.as_ptr(),
        )
    };
    if handle.is_null() {
        return Err(error());
    }
    let handle = Handle(handle);
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Err("Pageant mapping name collision.".into());
    }
    let view = View(unsafe {
        MapViewOfFile(handle.0, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, MAPPING_SIZE)
    });
    if view.0.Value.is_null() {
        return Err(error());
    }
    let bytes = view.0.Value.cast::<u8>();
    unsafe {
        ptr::copy_nonoverlapping((body.len() as u32).to_be_bytes().as_ptr(), bytes, 4);
        ptr::copy_nonoverlapping(body.as_ptr(), bytes.add(4), body.len());
    }
    let mut ansi_name = name.into_bytes();
    ansi_name.push(0);
    let request = COPYDATASTRUCT {
        dwData: 0x804e50ba,
        cbData: ansi_name.len() as u32,
        lpData: ansi_name.as_mut_ptr().cast(),
    };
    let mut result = 0;
    let sent = unsafe {
        SendMessageTimeoutW(
            window,
            WM_COPYDATA,
            0,
            &request as *const COPYDATASTRUCT as isize,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
            &mut result,
        )
    };
    if sent == 0 || result == 0 {
        return Err("Pageant declined or timed out.".into());
    }
    let length = u32::from_be_bytes(
        unsafe { std::slice::from_raw_parts(bytes, 4) }
            .try_into()
            .unwrap(),
    ) as usize;
    if length == 0 || length > MAPPING_SIZE - 4 {
        return Err("Invalid Pageant response size.".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(bytes.add(4), length) }.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_pageant_wait_discards_late_result_without_waiting_for_agent() {
        use std::sync::{atomic::AtomicBool, mpsc, Arc};
        let stop = Arc::new(AtomicBool::new(false));
        let cancellation = stop.clone();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (finished_tx, finished_rx) = mpsc::sync_channel(1);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = wait_cancellable(
                move || {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    finished_tx.send(()).unwrap();
                    Ok(vec![14, 0, 0, 0, 0])
                },
                &cancellation,
                Duration::from_secs(120),
            );
            result_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        stop.store(true, Ordering::Release);
        let result = result_rx.recv_timeout(Duration::from_secs(1));
        // Always release the mock agent, including if cancellation regresses.
        release_tx.send(()).unwrap();
        assert!(result.unwrap().unwrap_err().contains("cancelled"));
        finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn mapping_security_is_valid_and_has_a_protected_dacl() {
        let descriptor = current_user_security(false).unwrap();
        assert_ne!(unsafe { IsValidSecurityDescriptor(descriptor.0) }, 0);
        let (mut control, mut revision) = (0, 0);
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) },
            0
        );
        assert_ne!(control & SE_DACL_PROTECTED, 0);
        let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted)
            },
            0
        );
        assert_ne!(present, 0);
        assert!(!acl.is_null());
        assert_eq!(unsafe { (*acl).AceCount }, 1);
    }

    #[test]
    fn restricted_folders_pass_current_user_only_access_to_new_files() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = std::fs::OpenOptions::new()
            .access_mode(0x0004_0000 | 0x0002_0000) // WRITE_DAC | READ_CONTROL
            .custom_flags(0x0200_0000) // FILE_FLAG_BACKUP_SEMANTICS opens a folder
            .open(dir.path())
            .unwrap();
        restrict_folder(&folder).unwrap();
        let child = dir.path().join("saved by editor.txt");
        std::fs::write(&child, b"x").unwrap();
        let path = wide(child.to_str().unwrap());
        let (mut dacl, mut descriptor) = (ptr::null_mut(), ptr::null_mut());
        let status = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, 0);
        let _descriptor = Allocation(descriptor);
        // Exactly the folder's inherited entry, not the token's default DACL.
        assert_eq!(unsafe { (*dacl).AceCount }, 1);
        let mut ace = ptr::null_mut();
        assert_ne!(unsafe { GetAce(dacl, 0, &mut ace) }, 0);
        let header = unsafe { &*(ace as *const ACE_HEADER) };
        assert_ne!(u32::from(header.AceFlags) & INHERITED_ACE, 0);
    }

    unsafe extern "system" fn fixture_window(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_COPYDATA {
            let request = &*(lparam as *const COPYDATASTRUCT);
            if request.dwData != 0x804e50ba || request.cbData > 256 {
                return 0;
            }
            let name =
                std::slice::from_raw_parts(request.lpData.cast::<u8>(), request.cbData as usize);
            if name.last() != Some(&0) {
                return 0;
            }
            let name = wide(&String::from_utf8_lossy(&name[..name.len() - 1]));
            let mapping = Handle(OpenFileMappingW(
                FILE_MAP_READ | FILE_MAP_WRITE,
                0,
                name.as_ptr(),
            ));
            if mapping.0.is_null() {
                return 0;
            }
            let view = View(MapViewOfFile(
                mapping.0,
                FILE_MAP_READ | FILE_MAP_WRITE,
                0,
                0,
                MAPPING_SIZE,
            ));
            if view.0.Value.is_null() {
                return 0;
            }
            let bytes = view.0.Value.cast::<u8>();
            if std::slice::from_raw_parts(bytes, 5) != [0, 0, 0, 1, 11] {
                return 0;
            }
            ptr::copy_nonoverlapping([0u8, 0, 0, 5, 12, 0, 0, 0, 0].as_ptr(), bytes, 9);
            return 1;
        }
        if message == WM_CLOSE {
            DestroyWindow(window);
            return 0;
        }
        if message == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        DefWindowProcW(window, message, wparam, lparam)
    }

    #[test]
    fn pageant_round_trip_uses_accessible_private_mapping() {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || unsafe {
            let name = wide("ssh4-pageant-test");
            let class = WNDCLASSW {
                lpfnWndProc: Some(fixture_window),
                lpszClassName: name.as_ptr(),
                ..std::mem::zeroed()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let window = CreateWindowExW(
                0,
                name.as_ptr(),
                name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            );
            assert!(!window.is_null());
            tx.send(window as usize).unwrap();
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&message);
            }
            UnregisterClassW(name.as_ptr(), ptr::null_mut());
        });
        let window = rx.recv_timeout(Duration::from_secs(3)).unwrap() as HWND;
        let response = exchange_window(&[11], Duration::from_secs(2), window);
        unsafe {
            PostMessageW(window, WM_CLOSE, 0, 0);
        }
        worker.join().unwrap();
        assert_eq!(response.unwrap(), [12, 0, 0, 0, 0]);
    }
}
