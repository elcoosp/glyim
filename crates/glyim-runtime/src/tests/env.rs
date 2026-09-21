#![allow(unused_unsafe)]
use std::ptr;

#[test]
fn test_glyim_env_var_home() {
    unsafe {
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let result =
            crate::glyim_env_var("HOME".as_ptr(), "HOME".len(), &mut out_ptr, &mut out_len);
        // HOME may be unset on platforms where it is not conventionally defined
        // (e.g. Windows, which uses USERPROFILE). Tolerate both outcomes.
        if result >= 0 {
            assert!(!out_ptr.is_null(), "out_ptr should not be null on success");
            assert!(out_len > 0, "out_len should be > 0 for HOME");
            let home = std::slice::from_raw_parts(out_ptr, out_len);
            let home_str = std::str::from_utf8(home).expect("HOME should be valid UTF-8");
            assert!(!home_str.is_empty(), "HOME should not be empty");
            crate::glyim_free_cstr(out_ptr);
        } else {
            assert_eq!(result, -1, "HOME unset should return -1");
        }
    }
}

#[test]
fn test_glyim_env_var_nonexistent() {
    unsafe {
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let result = crate::glyim_env_var(
            "GLYIM_TEST_NONEXISTENT_VAR_12345".as_ptr(),
            "GLYIM_TEST_NONEXISTENT_VAR_12345".len(),
            &mut out_ptr,
            &mut out_len,
        );
        assert_eq!(
            result, -1,
            "glyim_env_var should return -1 for nonexistent var"
        );
        assert!(
            out_ptr.is_null(),
            "out_ptr should be null for nonexistent var"
        );
    }
}

#[test]
fn test_glyim_env_set_var() {
    unsafe {
        let result = crate::glyim_env_set_var(
            "GLYIM_TEST_SET_VAR".as_ptr(),
            "GLYIM_TEST_SET_VAR".len(),
            "test_value_123".as_ptr(),
            "test_value_123".len(),
        );
        assert_eq!(result, 0, "glyim_env_set_var should return 0 on success");

        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let get_result = crate::glyim_env_var(
            "GLYIM_TEST_SET_VAR".as_ptr(),
            "GLYIM_TEST_SET_VAR".len(),
            &mut out_ptr,
            &mut out_len,
        );
        assert_eq!(get_result, 0, "should be able to read the set var");
        let value = std::slice::from_raw_parts(out_ptr, out_len);
        let value_str = std::str::from_utf8(value).expect("value should be valid UTF-8");
        assert_eq!(value_str, "test_value_123");
        crate::glyim_free_cstr(out_ptr);

        crate::glyim_env_remove_var("GLYIM_TEST_SET_VAR".as_ptr(), "GLYIM_TEST_SET_VAR".len());
    }
}

#[test]
fn test_glyim_env_remove_var() {
    unsafe {
        crate::glyim_env_set_var(
            "GLYIM_TEST_REMOVE_VAR".as_ptr(),
            "GLYIM_TEST_REMOVE_VAR".len(),
            "to_be_removed".as_ptr(),
            "to_be_removed".len(),
        );

        let result = crate::glyim_env_remove_var(
            "GLYIM_TEST_REMOVE_VAR".as_ptr(),
            "GLYIM_TEST_REMOVE_VAR".len(),
        );
        assert_eq!(result, 0, "glyim_env_remove_var should return 0 on success");

        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let get_result = crate::glyim_env_var(
            "GLYIM_TEST_REMOVE_VAR".as_ptr(),
            "GLYIM_TEST_REMOVE_VAR".len(),
            &mut out_ptr,
            &mut out_len,
        );
        assert_eq!(get_result, -1, "var should be removed");
    }
}

#[test]
fn test_glyim_env_args_count() {
    unsafe {
        let count = crate::glyim_env_args_count();
        assert!(
            count >= 1,
            "should have at least one argument (program name)"
        );
    }
}

#[test]
fn test_glyim_env_args_get() {
    unsafe {
        let count = crate::glyim_env_args_count();
        if count > 0 {
            let mut out_ptr: *mut u8 = ptr::null_mut();
            let mut out_len: usize = 0;
            let result = crate::glyim_env_args_get(0, &mut out_ptr, &mut out_len);
            assert_eq!(result, 0, "should get first arg successfully");
            assert!(!out_ptr.is_null(), "out_ptr should not be null");
            assert!(out_len > 0, "out_len should be > 0");
            let arg = std::slice::from_raw_parts(out_ptr, out_len);
            let _arg_str = std::str::from_utf8(arg).expect("arg should be valid UTF-8");
            crate::glyim_free_cstr(out_ptr);
        }
    }
}

#[test]
fn test_glyim_env_args_get_out_of_bounds() {
    unsafe {
        let count = crate::glyim_env_args_count();
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let result = crate::glyim_env_args_get(count + 100, &mut out_ptr, &mut out_len);
        assert_eq!(result, -2, "should return -2 for out of bounds index");
    }
}

#[test]
fn test_glyim_env_current_dir() {
    unsafe {
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let result = crate::glyim_env_current_dir(&mut out_ptr, &mut out_len);
        assert_eq!(result, 0, "glyim_env_current_dir should succeed");
        assert!(!out_ptr.is_null(), "out_ptr should not be null");
        assert!(out_len > 0, "out_len should be > 0");
        let dir = std::slice::from_raw_parts(out_ptr, out_len);
        let dir_str = std::str::from_utf8(dir).expect("dir should be valid UTF-8");
        assert!(!dir_str.is_empty(), "current dir should not be empty");
        crate::glyim_free_cstr(out_ptr);
    }
}

#[test]
fn test_glyim_env_home_dir() {
    unsafe {
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let result = crate::glyim_env_home_dir(&mut out_ptr, &mut out_len);
        if result >= 0 {
            assert!(
                !out_ptr.is_null(),
                "out_ptr should not be null if HOME is set"
            );
            crate::glyim_free_cstr(out_ptr);
        }
    }
}

#[test]
fn test_glyim_env_temp_dir() {
    unsafe {
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let result = crate::glyim_env_temp_dir(&mut out_ptr, &mut out_len);
        assert_eq!(result, 0, "glyim_env_temp_dir should always succeed");
        assert!(!out_ptr.is_null(), "out_ptr should not be null");
        assert!(out_len > 0, "out_len should be > 0");
        crate::glyim_free_cstr(out_ptr);
    }
}

#[test]
fn test_glyim_env_current_exe() {
    unsafe {
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len: usize = 0;
        let result = crate::glyim_env_current_exe(&mut out_ptr, &mut out_len);
        assert_eq!(result, 0, "glyim_env_current_exe should succeed");
        assert!(!out_ptr.is_null(), "out_ptr should not be null");
        assert!(out_len > 0, "out_len should be > 0");
        crate::glyim_free_cstr(out_ptr);
    }
}
