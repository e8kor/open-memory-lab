//! Files only their owner opens: the `--out` key file ([`crate::issue`]) and
//! the stored credentials ([`crate::creds`]) each hold a key value, so each
//! is made owner-only before a byte of it is written (ADR-0028 d1;
//! cli/reference.md "Owner-only files").
//!
//! On Unix the file is mode 0600. On Windows it carries a protected DACL with
//! one entry, allowing the current user full access and inheriting nothing,
//! set on a handle opened with no sharing, so no other open of the file
//! succeeds while the CLI holds it. On any other platform [`restrict`]
//! refuses, so the CLI writes no key value there.

use std::fs::{File, OpenOptions};
use std::io;

/// Sets this platform's owner-only open flags on `options`: creation mode
/// 0600 on Unix (the umask can only narrow it); on Windows, no sharing, and
/// the `WRITE_DAC` right [`restrict`] needs on the handle.
pub fn open_options(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    #[cfg(windows)]
    windows::open_options(options);
    options
}

/// Makes the open `file` its owner's alone: mode 0600 exactly on Unix, and on
/// Windows the one-entry protected DACL above.
///
/// # Errors
///
/// The platform's refusal; on a platform that is neither Unix nor Windows,
/// always, so no key value is written to a file there.
pub fn restrict(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(windows)]
    {
        windows::restrict(file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the CLI makes a file owner-only on Unix and Windows alone, and this platform is \
             neither",
        ))
    }
}

/// The DACL a file carries, as the Windows tests read it back.
#[cfg(all(windows, test))]
pub(crate) use windows::{Dacl, dacl_of};

#[cfg(windows)]
// A reviewed exception to the workspace's `unsafe_code = "deny"`
// (workspace-plan.md, "Workspace manifest policy"): std has no call that sets
// a file's DACL, so these are the Win32 calls themselves, each checked, on
// buffers this module owns.
#[allow(unsafe_code)]
mod windows {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
    use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetSecurityInfo};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, DACL_SECURITY_INFORMATION,
        GetLengthSid, GetTokenInformation, InitializeAcl, PROTECTED_DACL_SECURITY_INFORMATION,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_ALL_ACCESS, FILE_GENERIC_WRITE, WRITE_DAC};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    pub(super) fn open_options(options: &mut OpenOptions) {
        // Share mode 0: until the CLI closes the handle, every other open of
        // the file fails with a sharing violation, so nothing opens it while
        // its DACL is still the one it inherited.
        options
            .share_mode(0)
            .access_mode(FILE_GENERIC_WRITE | WRITE_DAC);
    }

    pub(super) fn restrict(file: &File) -> io::Result<()> {
        let user = CurrentUser::of_this_process()?;
        let acl = OwnerOnlyAcl::for_sid(&user)?;
        // SAFETY: the handle is the open file's, valid for this call; the
        // ACL is a buffer InitializeAcl and AddAccessAllowedAce filled; no
        // owner, group or SACL is passed, so none is changed.
        let status = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                acl.as_ptr(),
                ptr::null(),
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(
                i32::try_from(status).unwrap_or(i32::MAX),
            ))
        }
    }

    /// The current process's user, as the `TOKEN_USER` its token reports;
    /// the SID points into `buffer`.
    pub(super) struct CurrentUser {
        buffer: Vec<u64>,
    }

    impl CurrentUser {
        pub(super) fn of_this_process() -> io::Result<Self> {
            let mut token: HANDLE = ptr::null_mut();
            // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no
            // closing; `token` receives a handle closed below.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let read = Self::read(token);
            // SAFETY: `token` is the handle OpenProcessToken opened above.
            unsafe { CloseHandle(token) };
            read
        }

        fn read(token: HANDLE) -> io::Result<Self> {
            let mut needed = 0_u32;
            // SAFETY: a null buffer of length 0 asks only for the length.
            unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &raw mut needed) };
            if needed == 0 {
                return Err(io::Error::last_os_error());
            }
            // u64 elements keep TOKEN_USER's pointer field aligned.
            let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
            let length = u32::try_from(buffer.len() * 8).map_err(io::Error::other)?;
            // SAFETY: `buffer` is writable for `length` bytes.
            let read = unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    length,
                    &raw mut needed,
                )
            };
            if read == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { buffer })
        }

        pub(super) fn sid(&self) -> *mut core::ffi::c_void {
            // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of
            // the aligned buffer, which outlives the returned pointer's uses.
            unsafe { (*self.buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        }
    }

    /// An ACL holding one entry: allow the given SID every file right.
    struct OwnerOnlyAcl {
        buffer: Vec<u32>,
    }

    impl OwnerOnlyAcl {
        fn for_sid(user: &CurrentUser) -> io::Result<Self> {
            let sid = user.sid();
            // SAFETY: `sid` is the valid SID the token reported.
            let sid_length = unsafe { GetLengthSid(sid) } as usize;
            // The ACL header, then one ACE whose SidStart field is the SID's
            // first four bytes (the size MSDN gives for AddAccessAllowedAce).
            let length =
                size_of::<ACL>() + size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>() + sid_length;
            let mut buffer = vec![0_u32; length.div_ceil(4)];
            let acl_length = u32::try_from(buffer.len() * 4).map_err(io::Error::other)?;
            let acl = buffer.as_mut_ptr().cast::<ACL>();
            // SAFETY: `acl` points at `acl_length` writable, DWORD-aligned
            // bytes, and `sid` is valid.
            unsafe {
                if InitializeAcl(acl, acl_length, ACL_REVISION) == 0
                    || AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid) == 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(Self { buffer })
        }

        fn as_ptr(&self) -> *const ACL {
            self.buffer.as_ptr().cast()
        }
    }

    /// A file's DACL as the tests check it: whether it is protected, and
    /// each entry's type, access mask, and whether its SID is the current
    /// user's.
    #[cfg(test)]
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct Dacl {
        pub(crate) protected: bool,
        pub(crate) entries: Vec<(u8, u32, bool)>,
    }

    #[cfg(test)]
    impl Dacl {
        /// What [`restrict`] sets: protected, and one entry allowing the
        /// current user every file right.
        pub(crate) fn owner_only() -> Self {
            use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
            Self {
                protected: true,
                entries: vec![(
                    u8::try_from(ACCESS_ALLOWED_ACE_TYPE).expect("a byte"),
                    FILE_ALL_ACCESS,
                    true,
                )],
            }
        }
    }

    /// Reads back the DACL of the file at `path`.
    #[cfg(test)]
    pub(crate) fn dacl_of(path: &std::path::Path) -> io::Result<Dacl> {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::GetNamedSecurityInfoW;
        use windows_sys::Win32::Security::{
            ACE_HEADER, EqualSid, GetAce, GetSecurityDescriptorControl, PSECURITY_DESCRIPTOR,
            SE_DACL_PROTECTED,
        };

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; the descriptor the call
        // allocates is freed with LocalFree below.
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut dacl,
                ptr::null_mut(),
                &raw mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(
                i32::try_from(status).unwrap_or(i32::MAX),
            ));
        }
        let user = CurrentUser::of_this_process();
        let read = user.and_then(|user| {
            let mut control = 0_u16;
            let mut revision = 0_u32;
            // SAFETY: `descriptor` and `dacl` are what the call above
            // returned; each ACE pointer GetAce gives points into `dacl`.
            unsafe {
                if GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision)
                    == 0
                {
                    return Err(io::Error::last_os_error());
                }
                let mut entries = Vec::new();
                for index in 0..(*dacl).AceCount {
                    let mut ace: *mut core::ffi::c_void = ptr::null_mut();
                    if GetAce(dacl, u32::from(index), &raw mut ace) == 0 {
                        return Err(io::Error::last_os_error());
                    }
                    let header = *ace.cast::<ACE_HEADER>();
                    let allowed = ace.cast::<ACCESS_ALLOWED_ACE>();
                    let sid = (&raw mut (*allowed).SidStart).cast();
                    entries.push((
                        header.AceType,
                        (*allowed).Mask,
                        EqualSid(sid, user.sid()) != 0,
                    ));
                }
                Ok(Dacl {
                    protected: control & SE_DACL_PROTECTED != 0,
                    entries,
                })
            }
        });
        // SAFETY: `descriptor` was allocated by GetNamedSecurityInfoW.
        unsafe { LocalFree(descriptor) };
        read
    }
}
