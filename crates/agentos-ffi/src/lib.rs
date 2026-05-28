use identity_core::{Ed25519CryptoProvider, SignedChallenge};
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

const AGENTOS_FFI_VERSION: &str = "0.1.0";
const ERR_NULL_ARGUMENT: i32 = -1;
const ERR_INVALID_UTF8: i32 = -2;
const ERR_IDENTITY_CORE: i32 = -3;
const ERR_PANIC: i32 = -255;

/// Returns the FFI library version as a borrowed null-terminated string.
/// The returned pointer is valid for the lifetime of the loaded dynamic library.
#[unsafe(no_mangle)]
pub extern "C" fn agentos_ffi_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}

/// Frees a string allocated by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agentos_ffi_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

/// Verifies an Ed25519 signature over a challenge using identity-core.
///
/// Return codes:
/// - 1: valid signature
/// - 0: well-formed but invalid signature
/// - negative: error; if error_out is not null, it receives an allocated string
///   that must be released with agentos_ffi_free_string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn agentos_identity_verify_ed25519_challenge(
    challenge: *const c_char,
    signature_hex: *const c_char,
    public_key_hex: *const c_char,
    error_out: *mut *mut c_char,
) -> i32 {
    if !error_out.is_null() {
        unsafe {
            *error_out = ptr::null_mut();
        }
    }

    match catch_unwind(AssertUnwindSafe(|| unsafe {
        verify_ed25519_challenge_inner(challenge, signature_hex, public_key_hex, error_out)
    })) {
        Ok(code) => code,
        Err(_) => {
            unsafe { set_error(error_out, "panic in agentos ffi") };
            ERR_PANIC
        }
    }
}

unsafe fn verify_ed25519_challenge_inner(
    challenge: *const c_char,
    signature_hex: *const c_char,
    public_key_hex: *const c_char,
    error_out: *mut *mut c_char,
) -> i32 {
    if challenge.is_null() || signature_hex.is_null() || public_key_hex.is_null() {
        unsafe { set_error(error_out, "null argument") };
        return ERR_NULL_ARGUMENT;
    }

    let challenge = match unsafe { cstr_to_str(challenge) } {
        Ok(value) => value,
        Err(err) => {
            unsafe { set_error(error_out, err) };
            return ERR_INVALID_UTF8;
        }
    };
    let signature_hex = match unsafe { cstr_to_str(signature_hex) } {
        Ok(value) => value,
        Err(err) => {
            unsafe { set_error(error_out, err) };
            return ERR_INVALID_UTF8;
        }
    };
    let public_key_hex = match unsafe { cstr_to_str(public_key_hex) } {
        Ok(value) => value,
        Err(err) => {
            unsafe { set_error(error_out, err) };
            return ERR_INVALID_UTF8;
        }
    };

    let signed = SignedChallenge {
        challenge: challenge.to_string(),
        signature: signature_hex.to_string(),
        public_key: public_key_hex.to_string(),
    };

    match Ed25519CryptoProvider::new().verify_challenge(&signed) {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(err) => {
            unsafe { set_error(error_out, &err.to_string()) };
            ERR_IDENTITY_CORE
        }
    }
}

unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Result<&'a str, &'static str> {
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "invalid utf-8 argument")
}

unsafe fn set_error(error_out: *mut *mut c_char, message: &str) {
    if error_out.is_null() {
        return;
    }
    let sanitized = message.replace('\0', " ");
    let c_string = CString::new(sanitized).expect("sanitized error message has no nul bytes");
    unsafe {
        *error_out = c_string.into_raw();
    }
}

pub fn version() -> &'static str {
    AGENTOS_FFI_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity_core::{CryptoProvider, Ed25519CryptoProvider};
    use std::ffi::CString;

    #[test]
    fn version_is_reported() {
        assert_eq!(version(), "0.1.0");
        let ptr = agentos_ffi_version();
        let version = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(version, "0.1.0");
    }

    #[test]
    fn verifies_valid_signature() {
        let provider = Ed25519CryptoProvider::new();
        let keypair = provider.generate_keypair().unwrap();
        let challenge = "ffi-challenge";
        let signed = provider
            .sign_challenge(challenge, &keypair.private_key, &keypair.public_key)
            .unwrap();

        let challenge = CString::new(challenge).unwrap();
        let signature = CString::new(signed.signature).unwrap();
        let public_key = CString::new(signed.public_key).unwrap();
        let mut err: *mut c_char = ptr::null_mut();

        let code = unsafe {
            agentos_identity_verify_ed25519_challenge(
                challenge.as_ptr(),
                signature.as_ptr(),
                public_key.as_ptr(),
                &mut err,
            )
        };

        assert_eq!(code, 1);
        assert!(err.is_null());
    }

    #[test]
    fn rejects_wrong_challenge() {
        let provider = Ed25519CryptoProvider::new();
        let keypair = provider.generate_keypair().unwrap();
        let signed = provider
            .sign_challenge("ffi-challenge", &keypair.private_key, &keypair.public_key)
            .unwrap();

        let challenge = CString::new("wrong-challenge").unwrap();
        let signature = CString::new(signed.signature).unwrap();
        let public_key = CString::new(signed.public_key).unwrap();
        let mut err: *mut c_char = ptr::null_mut();

        let code = unsafe {
            agentos_identity_verify_ed25519_challenge(
                challenge.as_ptr(),
                signature.as_ptr(),
                public_key.as_ptr(),
                &mut err,
            )
        };

        assert_eq!(code, 0);
        assert!(err.is_null());
    }

    #[test]
    fn returns_error_for_malformed_hex() {
        let challenge = CString::new("ffi-challenge").unwrap();
        let signature = CString::new("not-hex").unwrap();
        let public_key = CString::new("not-hex").unwrap();
        let mut err: *mut c_char = ptr::null_mut();

        let code = unsafe {
            agentos_identity_verify_ed25519_challenge(
                challenge.as_ptr(),
                signature.as_ptr(),
                public_key.as_ptr(),
                &mut err,
            )
        };

        assert_eq!(code, ERR_IDENTITY_CORE);
        assert!(!err.is_null());
        unsafe { agentos_ffi_free_string(err) };
    }
}
