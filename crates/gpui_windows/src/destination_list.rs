use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    path::{Path, PathBuf},
    sync::Arc,
};

use itertools::Itertools;
use smallvec::SmallVec;
use windows::{
    Win32::{
        Foundation::{E_ACCESSDENIED, PROPERTYKEY},
        Globalization::u_strlen,
        System::Com::{
            CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, StructuredStorage::PROPVARIANT,
        },
        UI::{
            Controls::INFOTIPSIZE,
            Shell::{
                Common::{IObjectArray, IObjectCollection},
                DestinationList, EnumerableObjectCollection,
                GetCurrentProcessExplicitAppUserModelID, ICustomDestinationList, IShellItem,
                IShellLinkW, KDC_RECENT,
                PropertiesSystem::IPropertyStore,
                SHARD_APPIDINFO, SHARD_PATHW, SHARDAPPIDINFO, SHAddToRecentDocs,
                SHCreateItemFromParsingName, ShellLink,
            },
        },
    },
    core::{GUID, HSTRING, Interface, PCWSTR},
};

use gpui::{Action, JumpListIcon, JumpListRecent, MenuItem, SharedString};

pub(crate) struct JumpList {
    pub(crate) dock_menus: Vec<DockMenuItem>,
    pub(crate) recent: Arc<JumpListRecent>,
}

impl JumpList {
    pub(crate) fn new() -> Self {
        Self {
            dock_menus: Vec::default(),
            recent: Arc::default(),
        }
    }
}

pub(crate) struct DockMenuItem {
    pub(crate) name: SharedString,
    pub(crate) description: SharedString,
    pub(crate) action: Box<dyn Action>,
}

impl DockMenuItem {
    pub(crate) fn new(item: MenuItem) -> anyhow::Result<Self> {
        match item {
            MenuItem::Action { name, action, .. } => Ok(Self {
                name: name.clone(),
                description: if name == "New Window" {
                    "Opens a new window".into()
                } else {
                    name
                },
                action,
            }),
            _ => anyhow::bail!("Only `MenuItem::Action` is supported for dock menu on Windows."),
        }
    }
}

// This code is based on the example from Microsoft:
// https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/Win7Samples/winui/shell/appshellintegration/RecipePropertyHandler/RecipePropertyHandler.cpp
pub(crate) fn update_jump_list(
    recent: &JumpListRecent,
    dock_menus: &[(SharedString, SharedString)],
) -> anyhow::Result<Vec<SmallVec<[PathBuf; 2]>>> {
    let (list, removed) = create_destination_list()?;
    add_recent(&list, recent, removed.as_ref())?;
    add_dock_menu(&list, dock_menus)?;
    unsafe { list.CommitList() }?;
    Ok(removed)
}

// Copied from:
// https://github.com/microsoft/windows-rs/blob/0fc3c2e5a13d4316d242bdeb0a52af611eba8bd4/crates/libs/windows/src/Windows/Win32/Storage/EnhancedStorage/mod.rs#L1881
const PKEY_TITLE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xf29f85e0_4ff9_1068_ab91_08002b27b3d9),
    pid: 2,
};

fn create_destination_list() -> anyhow::Result<(ICustomDestinationList, Vec<SmallVec<[PathBuf; 2]>>)>
{
    let list: ICustomDestinationList =
        unsafe { CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER) }?;

    let mut slots = 0;
    let user_removed: IObjectArray = unsafe { list.BeginList(&mut slots) }?;

    let count = unsafe { user_removed.GetCount() }?;
    if count == 0 {
        return Ok((list, Vec::new()));
    }

    let mut removed = Vec::with_capacity(count as usize);
    for i in 0..count {
        let shell_link: IShellLinkW = unsafe { user_removed.GetAt(i)? };
        let description = {
            // INFOTIPSIZE is the maximum size of the buffer
            // see https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishelllinkw-getdescription
            let mut buffer = [0u16; INFOTIPSIZE as usize];
            unsafe { shell_link.GetDescription(&mut buffer)? };
            let len = unsafe { u_strlen(buffer.as_ptr()) };
            String::from_utf16_lossy(&buffer[..len as usize])
        };
        let args = description.split('\n').map(PathBuf::from).collect();

        removed.push(args);
    }

    Ok((list, removed))
}

fn add_dock_menu(
    list: &ICustomDestinationList,
    dock_menus: &[(SharedString, SharedString)],
) -> anyhow::Result<()> {
    unsafe {
        let tasks: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        for (idx, (name, description)) in dock_menus.iter().enumerate() {
            let argument = HSTRING::from(format!("--dock-action {}", idx));
            let description = HSTRING::from(description.as_str());
            let display = name.as_str();
            let task = create_shell_link(argument, description, None, display)?;
            tasks.AddObject(&task)?;
        }
        list.AddUserTasks(&tasks)?;
        Ok(())
    }
}

fn add_recent(
    list: &ICustomDestinationList,
    recent: &JumpListRecent,
    removed: &Vec<SmallVec<[PathBuf; 2]>>,
) -> anyhow::Result<()> {
    unsafe {
        let tasks: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        let (icon, icon_index) = icon_location(&recent.icon)?;

        for folder_path in recent.entries.iter().filter(|path| !removed.contains(path)) {
            let argument = HSTRING::from(
                folder_path
                    .iter()
                    .map(|path| format!("\"{}\"", path.display()))
                    .join(" "),
            );

            let description = HSTRING::from(
                folder_path
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            let display = folder_path
                .iter()
                .map(|p| {
                    p.file_name()
                        .map(|name| name.to_string_lossy())
                        .unwrap_or_else(|| p.to_string_lossy())
                })
                .join(", ");

            tasks.AddObject(&create_shell_link(
                argument,
                description,
                Some((&icon, icon_index)),
                &display,
            )?)?;
        }

        if tasks.GetCount().unwrap_or(0) > 0 {
            match list.AppendCategory(&HSTRING::from(recent.title.as_str()), &tasks) {
                Ok(()) => {}
                // The user turned off recent items in jump lists. The tasks still go in.
                Err(error) if error.code() == E_ACCESSDENIED => {
                    log::info!("Windows declined the jump list's recent items: {error}");
                }
                Err(error) => return Err(error.into()),
            }
        }
        if recent.system_recent {
            list.AppendKnownCategory(KDC_RECENT)?;
        }
        Ok(())
    }
}

/// The file and index of the icon beside each recent entry.
fn icon_location(icon: &JumpListIcon) -> anyhow::Result<(HSTRING, i32)> {
    Ok(match icon {
        // simulate folder icon
        // https://github.com/microsoft/vscode/blob/7a5dc239516a8953105da34f84bae152421a8886/src/vs/platform/workspaces/electron-main/workspacesHistoryMainService.ts#L380
        JumpListIcon::Folder => (HSTRING::from("explorer.exe"), 0),
        JumpListIcon::App => (HSTRING::from(std::env::current_exe()?.as_os_str()), 0),
        JumpListIcon::File { path, index } => (HSTRING::from(path.as_os_str()), *index),
    })
}

/// Tells Windows that the document at `path` was used, for the user's Recent
/// items and the Recent category of the app's jump list.
pub(crate) fn add_recent_document(path: &Path) {
    let path = HSTRING::from(path.as_os_str());
    // A process with an explicit AppUserModelID names it, so that the document
    // counts for that app's jump list.
    if let Ok(app_id) = unsafe { GetCurrentProcessExplicitAppUserModelID() } {
        let added = add_recent_document_for_app(&path, PCWSTR(app_id.0));
        // SAFETY: Windows allocated the ID for this caller to free.
        unsafe { CoTaskMemFree(Some(app_id.0 as *const c_void)) };
        match added {
            Ok(()) => return,
            Err(error) => log::warn!("failed to add {path} to the app's recent documents: {error}"),
        }
    }
    // SAFETY: `path` is a null-terminated wide string that outlives the call.
    unsafe { SHAddToRecentDocs(SHARD_PATHW.0 as u32, Some(path.as_ptr().cast())) };
}

fn add_recent_document_for_app(path: &HSTRING, app_id: PCWSTR) -> windows::core::Result<()> {
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(path, None)? };
    let info = SHARDAPPIDINFO {
        psi: ManuallyDrop::new(Some(item)),
        pszAppID: app_id,
    };
    // SAFETY: `info` holds a live shell item and a null-terminated ID, both
    // outliving the call.
    unsafe {
        SHAddToRecentDocs(
            SHARD_APPIDINFO.0 as u32,
            Some(std::ptr::from_ref(&info).cast()),
        )
    };
    drop(ManuallyDrop::into_inner(info.psi));
    Ok(())
}

fn create_shell_link(
    argument: HSTRING,
    description: HSTRING,
    icon: Option<(&HSTRING, i32)>,
    display: &str,
) -> anyhow::Result<IShellLinkW> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let exe_path = HSTRING::from(std::env::current_exe()?.as_os_str());
        link.SetPath(&exe_path)?;
        link.SetArguments(&argument)?;
        link.SetDescription(&description)?;
        if let Some((icon, index)) = icon {
            link.SetIconLocation(icon, index)?;
        }
        let store: IPropertyStore = link.cast()?;
        let title = PROPVARIANT::from(display);
        store.SetValue(&PKEY_TITLE, &title)?;
        store.Commit()?;

        Ok(link)
    }
}
