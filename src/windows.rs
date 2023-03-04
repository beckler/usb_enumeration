use windows_sys::{
    core::GUID,
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
            SetupDiGetDeviceInstanceIdA, SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES,
            DIGCF_PRESENT, SPDRP_FRIENDLYNAME, SPDRP_HARDWAREID, SP_DEVINFO_DATA,
        },
        Foundation::{GetLastError, MAX_PATH},
    },
};

use crate::common::*;
use std::{
    error::Error,
    ffi::{CStr, OsStr},
    mem::size_of,
    os::windows::ffi::OsStrExt,
    ptr::{null, null_mut},
};

fn get_device_registry_property(
    dev_info: isize,
    devinfo_data: &mut SP_DEVINFO_DATA,
    property: u32,
) -> Option<String> {
    let mut buffer: Vec<u16> = vec![0; MAX_PATH as usize];

    if unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            dev_info,
            devinfo_data,
            property,
            null_mut(),
            buffer.as_mut_ptr() as *mut u8,
            buffer.len() as u32,
            null_mut(),
        )
    } < 1
    {
        return None;
    }

    // convert our buffer to a string
    String::from_utf16_lossy(&buffer)
        .trim_end_matches(0 as char)
        .split(';')
        .last()
        .map(str::to_string)
}

fn get_instance_id(dev_info: isize, devinfo_data: &mut SP_DEVINFO_DATA) -> Option<String> {
    let mut buffer: Vec<u8> = vec![0u8; MAX_PATH as usize];
    if unsafe {
        SetupDiGetDeviceInstanceIdA(
            dev_info,
            devinfo_data,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            null_mut(),
        )
    } < 1
    {
        // Try to retrieve hardware id property.
        get_device_registry_property(dev_info, devinfo_data, SPDRP_HARDWAREID)
    } else {
        Some(unsafe {
            CStr::from_ptr(buffer.as_ptr() as *mut i8)
                .to_string_lossy()
                .into_owned()
        })
    }
}

fn extract_vid_pid(
    hardware_id: &Option<String>,
) -> Result<(u16, u16), Box<dyn Error + Send + Sync>> {
    match hardware_id {
        Some(id) => {
            let vid = id.find("VID_").ok_or(ParseError)?;
            let pid = id.find("PID_").ok_or(ParseError)?;

            Ok((
                u16::from_str_radix(&id[vid + 4..vid + 8], 16)?,
                u16::from_str_radix(&id[pid + 4..pid + 8], 16)?,
            ))
        }
        None => Err(Box::new(ParseError)),
    }
}

fn extract_serial_number(hardware_id: String) -> Option<String> {
    hardware_id.split("\\").last().map(|s| s.to_owned())
}

pub fn enumerate_platform(vid: Option<u16>, pid: Option<u16>) -> Vec<UsbDevice> {
    let mut output: Vec<UsbDevice> = Vec::new();
    let usb_enum: Vec<u16> = OsStr::new("USB\0").encode_wide().collect();

    // collect all usb devices
    let dev_info = unsafe {
        SetupDiGetClassDevsW(
            null(),
            usb_enum.as_ptr(),
            0,
            DIGCF_ALLCLASSES | DIGCF_PRESENT,
        )
    };

    // this is the shared struct that is use in each interation for the enumeration of usb devices
    let mut devinfo_data = SP_DEVINFO_DATA {
        cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
        ClassGuid: GUID::from_u128(0),
        DevInst: 0,
        Reserved: 0,
    };

    let mut i = 0;
    while unsafe { SetupDiEnumDeviceInfo(dev_info, i, &mut devinfo_data) } > 0 {
        i += 1;

        // get the hardward instance id
        let hardware_id = get_instance_id(dev_info, &mut devinfo_data);

        // validate the hardware id and extract info
        match extract_vid_pid(&hardware_id) {
            Ok((vendor_id, product_id)) => output.push(UsbDevice {
                id: hardware_id.unwrap().clone(),
                vendor_id,
                product_id,
                description: get_device_registry_property(
                    dev_info,
                    &mut devinfo_data,
                    SPDRP_FRIENDLYNAME,
                ),
                serial_number: extract_serial_number(hardware_id.unwrap()),
            }),
            Err(_) => todo!(),
        }
    }

    unsafe { SetupDiDestroyDeviceInfoList(dev_info) };

    output
}
