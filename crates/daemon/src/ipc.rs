use shared_contracts::{IpcMessage, MessageType};
use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{error, warn};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    FlushFileBuffers, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

const PIPE_SECURITY_DESCRIPTOR_SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;LS)(A;;GA;;;IU)";

struct OwnedSecurityDescriptor(*mut core::ffi::c_void);

impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

pub trait IpcRequestHandler: Send + Sync + 'static {
    fn handle(&self, request: IpcMessage) -> IpcMessage;

    fn on_transport_error(&self, _message: &str) {}
}

pub fn spawn_server<H>(
    pipe_name: String,
    handler: Arc<H>,
    stop_requested: Arc<AtomicBool>,
) -> JoinHandle<()>
where
    H: IpcRequestHandler,
{
    thread::spawn(move || {
        while !stop_requested.load(Ordering::SeqCst) {
            let handle = match create_pipe_instance(&pipe_name) {
                Ok(handle) => handle,
                Err(error) => {
                    error!("failed to create pipe instance: {error}");
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }
            };

            if !connect_pipe(handle) {
                unsafe {
                    CloseHandle(handle);
                }
                if stop_requested.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
                continue;
            }

            if let Err(error) = process_client(handle, &*handler) {
                warn!("pipe client handling error: {error}");
                handler.on_transport_error(&error);
            }

            unsafe {
                DisconnectNamedPipe(handle);
                CloseHandle(handle);
            }
        }
    })
}

fn create_pipe_instance(pipe_name: &str) -> Result<HANDLE, String> {
    let name = to_wide(pipe_name);
    let (mut security_attributes, _descriptor) = build_pipe_security_attributes()?;
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            16 * 1024,
            16 * 1024,
            0,
            &mut security_attributes,
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        let code = unsafe { GetLastError() };
        return Err(format!("CreateNamedPipeW failed with code {code}"));
    }

    Ok(handle)
}

fn build_pipe_security_attributes() -> Result<(SECURITY_ATTRIBUTES, OwnedSecurityDescriptor), String>
{
    let sddl = to_wide(PIPE_SECURITY_DESCRIPTOR_SDDL);
    let mut security_descriptor = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut security_descriptor,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        return Err(format!(
            "ConvertStringSecurityDescriptorToSecurityDescriptorW failed with code {code}"
        ));
    }

    let attrs = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security_descriptor,
        bInheritHandle: 0,
    };

    Ok((attrs, OwnedSecurityDescriptor(security_descriptor)))
}

fn connect_pipe(handle: HANDLE) -> bool {
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected != 0 {
        return true;
    }

    let error_code = unsafe { GetLastError() };
    error_code == ERROR_PIPE_CONNECTED
}

fn process_client<H: IpcRequestHandler>(handle: HANDLE, handler: &H) -> Result<(), String> {
    let mut request_line = Vec::<u8>::new();
    loop {
        let mut chunk = [0u8; 4096];
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                handle,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        };

        if ok == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_BROKEN_PIPE {
                return Ok(());
            }
            return Err(format!("ReadFile failed with code {code}"));
        }

        if read == 0 {
            break;
        }

        request_line.extend_from_slice(&chunk[..read as usize]);
        if request_line.contains(&b'\n') {
            break;
        }
        if request_line.len() > 64 * 1024 {
            return Err("request exceeds 64 KiB".to_string());
        }
    }

    let line = std::str::from_utf8(&request_line)
        .map_err(|error| format!("request was not valid utf-8: {error}"))?
        .lines()
        .next()
        .unwrap_or_default()
        .trim();
    if line.is_empty() {
        return Ok(());
    }

    let response = match IpcMessage::from_line(line) {
        Ok(request) if request.message_type == MessageType::Request => handler.handle(request),
        Ok(request) => IpcMessage::error_response(&request, 400, "expected request message type"),
        Err(error) => IpcMessage {
            id: None,
            message_type: MessageType::Response,
            method: "UNKNOWN".to_string(),
            payload: serde_json::Value::Null,
            error: Some(shared_contracts::IpcError {
                code: 400,
                message: format!("invalid request json: {error}"),
            }),
        },
    };

    let line = response
        .to_line()
        .map_err(|error| format!("failed to serialize response: {error}"))?;
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            handle,
            line.as_bytes().as_ptr().cast(),
            line.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        return Err(format!("WriteFile failed with code {code}"));
    }

    unsafe {
        FlushFileBuffers(handle);
    }
    Ok(())
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
