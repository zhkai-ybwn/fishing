use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use windows::{
    Win32::{
        Foundation::{E_UNEXPECTED, ERROR_CANCELLED},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
        },
        UI::{
            Input::KeyboardAndMouse::GetActiveWindow,
            Shell::{
                Common::COMDLG_FILTERSPEC, FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST,
                FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FileOpenDialog,
                IFileOpenDialog, SIGDN_FILESYSPATH,
            },
        },
    },
    core::{Error, HRESULT, Result, w},
};

/// Open the native portrait chooser on the calling UI thread.
///
/// Returns `Ok(None)` when the user dismisses the chooser.
pub fn choose_portrait() -> std::result::Result<Option<PathBuf>, String> {
    select_file().map_err(|error| format!("无法选择照片（{error}）。"))
}

fn select_file() -> Result<Option<PathBuf>> {
    let _apartment = DialogApartment::initialize()?;
    let dialog = portrait_dialog()?;
    // SAFETY: The dialog and optional owner belong to the calling UI thread.
    // A successful GetDisplayName returns a null-terminated, COM-allocated path.
    unsafe {
        let owner = GetActiveWindow();
        match dialog.Show((!owner.0.is_null()).then_some(owner)) {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        let selected = dialog.GetResult()?;
        let path = selected.GetDisplayName(SIGDN_FILESYSPATH)?;
        if path.is_null() {
            return Err(Error::from_hresult(E_UNEXPECTED));
        }
        let result = PathBuf::from(OsString::from_wide(path.as_wide()));
        CoTaskMemFree(Some(path.0.cast()));
        Ok(Some(result))
    }
}

fn portrait_dialog() -> Result<IFileOpenDialog> {
    // SAFETY: The calling thread has initialized COM. Filter strings are static
    // and the file dialog copies options and filters during the calls.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let options = dialog.GetOptions()?
            | FOS_FORCEFILESYSTEM
            | FOS_FILEMUSTEXIST
            | FOS_PATHMUSTEXIST
            | FOS_NOCHANGEDIR
            | FOS_DONTADDTORECENT;
        dialog.SetOptions(options)?;
        dialog.SetTitle(w!("选择用于录入的人脸照片"))?;
        dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: w!("照片（JPG、JPEG、PNG）"),
            pszSpec: w!("*.jpg;*.jpeg;*.png"),
        }])?;
        Ok(dialog)
    }
}

struct DialogApartment;

impl DialogApartment {
    fn initialize() -> Result<Self> {
        // SAFETY: We balance this successful initialization, including S_FALSE,
        // with CoUninitialize on the calling UI thread.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()? };
        Ok(Self)
    }
}

impl Drop for DialogApartment {
    fn drop(&mut self) {
        // SAFETY: The matching successful initialization happened on this thread.
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_dialog_preserves_directory_and_recent_documents() {
        let _apartment = DialogApartment::initialize().unwrap();
        let dialog = portrait_dialog().unwrap();
        // SAFETY: The test's dialog was initialized on the current thread.
        let options = unsafe { dialog.GetOptions().unwrap() };
        let expected = FOS_NOCHANGEDIR | FOS_DONTADDTORECENT | FOS_FORCEFILESYSTEM;
        assert_eq!(options.0 & expected.0, expected.0);
    }
}
