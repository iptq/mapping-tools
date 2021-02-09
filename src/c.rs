use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::hitsounds::{CopyHitsoundOpts, ExtraOpts, copy_hitsounds_cmd};

fn convert_path(path: *const c_char) -> Result<PathBuf> {
    let path_cstr = unsafe { CStr::from_ptr(path) };
    let path_bytes = path_cstr.to_bytes();
    let path_str = String::from_utf8(path_bytes.to_vec())?;
    Ok(PathBuf::from(path_str))
}

#[no_mangle]
pub extern "C" fn mt_copy_hitsounds_cmd(src_path: *const c_char, dst_path: *const c_char) {
    let src_path = convert_path(src_path).unwrap();
    let dst_path = convert_path(dst_path).unwrap();

    let opts = CopyHitsoundOpts {
        src: src_path,
        dsts: vec![dst_path],
        extra: ExtraOpts::default(),
    };
    copy_hitsounds_cmd(opts).unwrap();
}
