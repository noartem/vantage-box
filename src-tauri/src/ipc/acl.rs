//! Windows named-pipe ACL: grant access to the current user and LocalSystem
//! only.
//!
//! The app installs per-user (`installMode: currentUser`), so we tighten the
//! pipe more than the generic "Authenticated Users" default from the original
//! ACTIONS.md draft: no other account — not even another admin on the machine
//! — can reach the control bus. Only the user who owns this Vantage Box
//! instance and the operating system itself.
//!
//! tokio's `ServerOptions` has no `security_attributes` builder, so we follow the
//! pattern its own docs prescribe: create each pipe instance with the default
//! descriptor, then set our DACL on it with `SetSecurityInfo` before accepting a
//! client. `write_dac(true)` is what lets that call succeed. `reject_remote_clients`
//! (set in `pipe.rs`) blocks SMB clients at the pipe layer as defense-in-depth.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    BuildTrusteeWithSidW, SetEntriesInAclW, SetSecurityInfo, EXPLICIT_ACCESS_W, GRANT_ACCESS,
    SE_KERNEL_OBJECT, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    CreateWellKnownSid, GetTokenInformation, TokenUser, WinLocalSystemSid, ACL,
    DACL_SECURITY_INFORMATION, PSID, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// `GENERIC_READ | GENERIC_WRITE` — what a named-pipe client needs to talk to
/// the server. Generic rights map to the pipe-specific rights at access check.
const PIPE_CLIENT_ACCESS: u32 = 0x8000_0000 | 0x4000_0000;
/// `GENERIC_ALL` for LocalSystem.
const SYSTEM_FULL_ACCESS: u32 = 0x1000_0000;

/// `SECURITY_MAX_SID_SIZE`.
const MAX_SID_SIZE: usize = 68;

/// Owns the DACL for the control pipe. Built once and applied to every pipe
/// instance tokio creates in the accept loop.
pub struct PipeAcl {
    acl: *mut ACL,
}

impl PipeAcl {
    /// Builds the ACL (current user read/write + LocalSystem full).
    pub fn build() -> Result<Self, String> {
        unsafe {
            // 1. Current user SID from our own process token.
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(format!("OpenProcessToken failed: {}", last_error()));
            }
            let user_sid = scope_user_sid(token).inspect_err(|_| {
                CloseHandle(token);
            })?;
            CloseHandle(token);

            // 2. LocalSystem well-known SID.
            let mut system_sid = vec![0u8; MAX_SID_SIZE];
            let mut system_len: u32 = system_sid.len() as u32;
            if CreateWellKnownSid(
                WinLocalSystemSid,
                null_mut(),
                system_sid.as_mut_ptr() as PSID,
                &mut system_len,
            ) == 0
            {
                return Err(format!("CreateWellKnownSid failed: {}", last_error()));
            }
            let system_sid_ptr = system_sid.as_mut_ptr() as PSID;

            // 3. Two grant ACEs: SYSTEM has full control, the user may read/write.
            let mut user_trustee: TRUSTEE_W = std::mem::zeroed();
            BuildTrusteeWithSidW(&mut user_trustee, user_sid.ptr);
            let mut system_trustee: TRUSTEE_W = std::mem::zeroed();
            BuildTrusteeWithSidW(&mut system_trustee, system_sid_ptr);

            let entries: [EXPLICIT_ACCESS_W; 2] = [
                EXPLICIT_ACCESS_W {
                    grfAccessPermissions: SYSTEM_FULL_ACCESS,
                    grfAccessMode: GRANT_ACCESS,
                    grfInheritance: 0,
                    Trustee: system_trustee,
                },
                EXPLICIT_ACCESS_W {
                    grfAccessPermissions: PIPE_CLIENT_ACCESS,
                    grfAccessMode: GRANT_ACCESS,
                    grfInheritance: 0,
                    Trustee: user_trustee,
                },
            ];

            // SetEntriesInAclW copies the SIDs into the ACEs, so the SID
            // buffers above can go out of scope after this returns.
            let mut acl: *mut ACL = null_mut();
            let err = SetEntriesInAclW(2, entries.as_ptr(), null_mut(), &mut acl);
            if err != ERROR_SUCCESS {
                return Err(format!("SetEntriesInAclW failed: code {err}"));
            }
            if acl.is_null() {
                return Err("SetEntriesInAclW returned a null ACL".into());
            }

            Ok(Self { acl })
        }
    }

    /// Applies this DACL to a named-pipe instance handle. Call after
    /// `ServerOptions::create`, before `connect`. Owner/group are left unset, so
    /// the kernel fills them from the creating process token (the current user).
    ///
    /// # Safety
    /// `handle` must be a valid named-pipe handle obtained from tokio.
    pub unsafe fn apply_to(&self, handle: HANDLE) -> Result<(), String> {
        let err = SetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            self.acl,
            null_mut(),
        );
        if err != ERROR_SUCCESS {
            return Err(format!("SetSecurityInfo failed: code {err}"));
        }
        Ok(())
    }
}

impl Drop for PipeAcl {
    fn drop(&mut self) {
        unsafe {
            if !self.acl.is_null() {
                LocalFree(self.acl as *mut std::ffi::c_void);
            }
        }
    }
}

// The ACL pointer is owned and not shared across threads after build(); the
// accept loop holds one PipeAcl and only reads it.
unsafe impl Send for PipeAcl {}
unsafe impl Sync for PipeAcl {}

/// The user SID extracted from a process token, kept alive with its backing
/// buffer for as long as the SID pointer is needed (during `build`).
struct UserSid {
    _buf: Vec<u8>,
    ptr: PSID,
}

/// Reads `TokenUser` from `token` and returns the SID pointer, keeping the
/// backing buffer alive in the returned `UserSid`.
unsafe fn scope_user_sid(token: HANDLE) -> Result<UserSid, String> {
    let mut len: u32 = 0;
    // First call fails with ERROR_INSUFFICIENT_BUFFER but tells us the size.
    GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
    if len == 0 {
        return Err(format!(
            "GetTokenInformation sizing failed: {}",
            last_error()
        ));
    }

    let mut buf = vec![0u8; len as usize];
    let ok = GetTokenInformation(
        token,
        TokenUser,
        buf.as_mut_ptr() as *mut std::ffi::c_void,
        len,
        &mut len,
    );
    if ok == 0 {
        return Err(format!("GetTokenInformation failed: {}", last_error()));
    }

    // `TOKEN_USER { User: SID_AND_ATTRIBUTES { Sid: PSID, Attributes: u32 } }`.
    // The SID itself is appended inside the same buffer, so `ptr` is valid only
    // while `buf` lives — which is why we return `UserSid` holding both.
    let sid = *(buf.as_ptr() as *const *const std::ffi::c_void);
    Ok(UserSid {
        _buf: buf,
        ptr: sid as PSID,
    })
}

fn last_error() -> String {
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    format!("Win32 error {code}")
}
