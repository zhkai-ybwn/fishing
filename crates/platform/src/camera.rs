use std::{ptr, slice};

use nokhwa::{
    pixel_format::RgbFormat,
    utils::{
        ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
    },
};
use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Media::MediaFoundation::{
            IMFActivate, MF_API_VERSION, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MFCreateAttributes,
            MFEnumDeviceSources, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup,
        },
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
    },
    core::{Error, GUID, PWSTR},
};

const BACKEND: ApiBackend = ApiBackend::MediaFoundation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraInfo {
    pub id: String,
    pub name: String,
}

pub struct Frame {
    pub rgb: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct Camera(nokhwa::Camera);

pub fn cameras() -> Result<Vec<CameraInfo>, String> {
    enumerate_devices().map_err(|error| format!("无法列出摄像头（{error}）。"))
}

fn enumerate_devices() -> windows::core::Result<Vec<CameraInfo>> {
    // The camera worker owns nokhwa's capture session. Enumeration balances its
    // own Media Foundation and COM calls on whichever thread requested the list.
    let _media_foundation = EnumerationSession::new()?;
    let mut attributes = None;
    let mut devices = ptr::null_mut();
    let mut count = 0;
    // SAFETY: All outputs are writable. Media Foundation returns a CoTaskMem
    // array of COM references, which we release before freeing the array.
    unsafe {
        MFCreateAttributes(&mut attributes, 1)?;
        let attributes = attributes.ok_or_else(Error::empty)?;
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;
        MFEnumDeviceSources(&attributes, &mut devices, &mut count)?;
        let found = if count > 0 && !devices.is_null() {
            let devices = slice::from_raw_parts_mut(devices, count as usize);
            let result = devices.iter().flatten().map(camera_info).collect();
            for device in devices {
                *device = None;
            }
            result
        } else {
            Ok(Vec::new())
        };
        CoTaskMemFree(Some(devices.cast()));
        found
    }
}

fn camera_info(device: &IMFActivate) -> windows::core::Result<CameraInfo> {
    Ok(CameraInfo {
        id: camera_attribute(
            device,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
        )?,
        name: camera_attribute(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)?,
    })
}

fn camera_attribute(device: &IMFActivate, key: &GUID) -> windows::core::Result<String> {
    let mut value = PWSTR::null();
    let mut length = 0;
    // SAFETY: Win32 returns a CoTaskMem string with the number of UTF-16 code
    // units, excluding its terminator. Borrow only while the allocation lives.
    unsafe {
        device.GetAllocatedString(key, &mut value, &mut length)?;
        let string = if length > 0 && !value.is_null() {
            String::from_utf16_lossy(slice::from_raw_parts(value.0, length as usize))
        } else {
            String::new()
        };
        CoTaskMemFree(Some(value.0.cast()));
        Ok(string)
    }
}

struct EnumerationSession {
    owns_com_reference: bool,
}

impl EnumerationSession {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: Every successful initialization is paired with uninitialize
        // on the same thread. RPC_E_CHANGED_MODE indicates COM is already active.
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if com != RPC_E_CHANGED_MODE {
            com.ok()?;
        }
        let owns_com_reference = com.is_ok();
        // SAFETY: This independent startup is paired with MFShutdown in Drop.
        if let Err(error) = unsafe { MFStartup(MF_API_VERSION, MFSTARTUP_NOSOCKET) } {
            if owns_com_reference {
                // SAFETY: The matching CoInitializeEx succeeded on this thread.
                unsafe { CoUninitialize() };
            }
            return Err(error);
        }
        Ok(Self { owns_com_reference })
    }
}

impl Drop for EnumerationSession {
    fn drop(&mut self) {
        // SAFETY: This stack-scoped session shuts down on its initiating thread.
        unsafe {
            let _ = MFShutdown();
            if self.owns_com_reference {
                CoUninitialize();
            }
        }
    }
}

impl Camera {
    pub fn open(id: &str) -> Result<Self, String> {
        let index = if id.is_empty() {
            CameraIndex::Index(0)
        } else {
            CameraIndex::String(id.to_owned())
        };
        let preferred = CameraFormat::new_from(1280, 720, FrameFormat::MJPEG, 30);
        let requested = RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(preferred));
        let mut camera = nokhwa::Camera::with_backend(index.clone(), requested, BACKEND)
            .or_else(|_| {
                let fallback = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
                nokhwa::Camera::with_backend(index, fallback, BACKEND)
            })
            .map_err(|error| format!("无法打开摄像头，请检查设备和系统相机权限（{error}）。"))?;
        if !id.is_empty() && camera.info().misc() != id {
            return Err("摄像头列表发生变化，请重新选择设备。".into());
        }
        camera.open_stream().map_err(|error| {
            format!("无法开始采集，请检查摄像头是否被其他应用占用（{error}）。")
        })?;

        Ok(Self(camera))
    }

    pub fn frame(&mut self) -> Result<Frame, String> {
        let frame = self
            .0
            .frame()
            .map_err(|error| format!("无法读取摄像头画面（{error}）。"))?;
        let image = frame
            .decode_image::<RgbFormat>()
            .map_err(|error| format!("无法解码摄像头画面（{error}）。"))?;
        let (width, height) = image.dimensions();
        Ok(Frame {
            rgb: image.into_raw(),
            width,
            height,
        })
    }
}
