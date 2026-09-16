use std::path::Path;
use stream_archive_server::environment_settings::SettingKind;
use windows::{
    Win32::{
        Foundation::ERROR_CANCELLED,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
            FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
            SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
        },
    },
    core::{HRESULT, HSTRING, Result, w},
};

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: created only after successful CoInitializeEx on this thread;
        // dialog/interface locals are dropped before this guard.
        unsafe { CoUninitialize() };
    }
}

/// Called on the GUI worker, never the Slint event thread. No subprocess or HTTP
/// bridge is used. COM interfaces and allocations remain on this STA thread.
pub fn pick(kind: SettingKind, initial: &str) -> Result<Option<String>> {
    // SAFETY: this worker owns its STA apartment and all COM objects below.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let _apartment = Apartment;
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let mut options =
            dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_DONTADDTORECENT;
        if kind == SettingKind::Directory {
            options |= FOS_PICKFOLDERS;
            dialog.SetTitle(w!("Stream Archive - Select folder"))?;
        } else {
            options |= FOS_FILEMUSTEXIST;
            dialog.SetTitle(w!("Stream Archive - Select executable"))?;
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: w!("Windows executable"),
                pszSpec: w!("*.exe;*.com"),
            }])?;
        }
        dialog.SetOptions(options)?;
        let path = Path::new(initial);
        let folder = if path.is_dir() {
            Some(path)
        } else {
            path.parent().filter(|p| p.is_dir())
        };
        if let Some(folder) = folder {
            let name = HSTRING::from(folder.as_os_str());
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&name, None) {
                dialog.SetFolder(&item)?;
            }
        }
        if let Err(error) = dialog.Show(None) {
            if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(None);
            }
            return Err(error);
        }
        let selected = dialog.GetResult()?;
        let allocated = selected.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = allocated.to_string();
        CoTaskMemFree(Some(allocated.0.cast()));
        Ok(Some(path?))
    }
}
