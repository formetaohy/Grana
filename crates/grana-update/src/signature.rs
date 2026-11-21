use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::Cryptography::{
    CERT_CONTEXT, CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW,
};
use windows_sys::Win32::Security::WinTrust::{
    CRYPT_PROVIDER_CERT, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvCertFromChain,
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
};

use crate::Error;

pub(crate) fn admits(current: &Path, offered: &Path) -> Result<(), Error> {
    let Some(required) = publisher(current) else {
        return Ok(());
    };
    let path = offered.display().to_string();
    match publisher(offered) {
        Some(actual) if actual.eq_ignore_ascii_case(&required) => Ok(()),
        Some(actual) => Err(Error::Signature {
            path,
            detail: format!("the release is signed by `{actual}` instead of `{required}`"),
        }),
        None => Err(Error::Signature {
            path,
            detail: "the release carries no trusted signature".to_string(),
        }),
    }
}

fn publisher(path: &Path) -> Option<String> {
    let file = wide(path.as_os_str());
    let mut info: WINTRUST_FILE_INFO = unsafe { std::mem::zeroed() };
    info.cbStruct = size_of::<WINTRUST_FILE_INFO>() as u32;
    info.pcwszFilePath = file.as_ptr();
    let mut data: WINTRUST_DATA = unsafe { std::mem::zeroed() };
    data.cbStruct = size_of::<WINTRUST_DATA>() as u32;
    data.dwUIChoice = WTD_UI_NONE;
    data.fdwRevocationChecks = WTD_REVOKE_NONE;
    data.dwUnionChoice = WTD_CHOICE_FILE;
    data.Anonymous.pFile = &mut info;
    data.dwStateAction = WTD_STATEACTION_VERIFY;
    data.dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = verify(&mut action, &mut data);
    let name = if status == 0 {
        subject(data.hWVTStateData)
    } else {
        None
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    verify(&mut action, &mut data);
    name
}

fn verify(action: &mut windows_sys::core::GUID, data: &mut WINTRUST_DATA) -> i32 {
    unsafe {
        WinVerifyTrust(
            std::ptr::null_mut(),
            action,
            (data as *mut WINTRUST_DATA).cast(),
        )
    }
}

fn subject(state: HANDLE) -> Option<String> {
    if state.is_null() {
        return None;
    }
    unsafe {
        let data = WTHelperProvDataFromStateData(state);
        if data.is_null() {
            return None;
        }
        let signer = WTHelperGetProvSignerFromChain(data, 0, 0, 0);
        if signer.is_null() {
            return None;
        }
        let provider: *const CRYPT_PROVIDER_CERT = WTHelperGetProvCertFromChain(signer, 0);
        if provider.is_null() {
            return None;
        }
        let context: *const CERT_CONTEXT = (*provider).pCert;
        if context.is_null() {
            return None;
        }
        display_name(context)
    }
}

fn display_name(context: *const CERT_CONTEXT) -> Option<String> {
    let length = unsafe {
        CertGetNameStringW(
            context,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        )
    };
    if length <= 1 {
        return None;
    }
    let mut buffer = vec![0u16; length as usize];
    let written = unsafe {
        CertGetNameStringW(
            context,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            std::ptr::null(),
            buffer.as_mut_ptr(),
            length,
        )
    };
    if written <= 1 {
        return None;
    }
    String::from_utf16(&buffer[..(written - 1) as usize]).ok()
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}
