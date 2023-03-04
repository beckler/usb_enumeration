use windows_sys::{
    core::GUID,
    Win32::{
        Devices::{
            DeviceAndDriverInstallation::{
                SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
                SetupDiGetDeviceInstanceIdA, SetupDiGetDevicePropertyW,
                SetupDiGetDeviceRegistryPropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT,
                SPDRP_FRIENDLYNAME, SPDRP_HARDWAREID, SP_DEVINFO_DATA,
            },
            Properties::{DEVPKEY_Device_BusReportedDeviceDesc, DEVPROPKEY},
        },
        Foundation::MAX_PATH,
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

fn get_device_property(
    dev_info: isize,
    devinfo_data: &mut SP_DEVINFO_DATA,
    property: &DEVPROPKEY,
) -> Option<String> {
    let mut buffer: Vec<u16> = vec![0u16; MAX_PATH as usize];
    let mut output_type: u32 = 0u32;
    let mut required_size: u32 = 0u32;
    if unsafe {
        SetupDiGetDevicePropertyW(
            dev_info,
            devinfo_data,
            property,
            &mut output_type,
            buffer.as_mut_ptr() as *mut u8,
            buffer.len() as u32,
            &mut required_size,
            0,
        )
    } < 1
    {
        return None;
    };

    // convert our buffer to a string
    Some(
        String::from_utf16_lossy(&buffer[0..required_size as usize])
            .trim_end_matches(0 as char)
            .to_string(),
    )
}

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

fn extract_vid_pid(hardware_id: &String) -> Result<(u16, u16), Box<dyn Error + Send + Sync>> {
    let vid = hardware_id.find("VID_").ok_or(ParseError)?;
    let pid = hardware_id.find("PID_").ok_or(ParseError)?;

    Ok((
        u16::from_str_radix(&hardware_id[vid + 4..vid + 8], 16)?,
        u16::from_str_radix(&hardware_id[pid + 4..pid + 8], 16)?,
    ))
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
        match get_instance_id(dev_info, &mut devinfo_data) {
            Some(hardware_id) => {
                // validate the hardware id
                if let Ok((vendor_id, product_id)) = extract_vid_pid(&hardware_id) {
                    if let Some(vid) = vid {
                        if vid != vendor_id {
                            continue;
                        }
                    }

                    if let Some(pid) = pid {
                        if pid != product_id {
                            continue;
                        }
                    }

                    // get the description - if there is no description, attempt to get the bus description
                    let description = get_device_registry_property(
                        dev_info,
                        &mut devinfo_data,
                        SPDRP_FRIENDLYNAME,
                    )
                    .or(get_device_property(
                        dev_info,
                        &mut devinfo_data,
                        &DEVPKEY_Device_BusReportedDeviceDesc,
                    ));

                    // add the device to our output
                    output.push(UsbDevice {
                        id: hardware_id.clone(),
                        vendor_id,
                        product_id,
                        description,
                        serial_number: extract_serial_number(hardware_id),
                    });
                }
            }
            None => (), // do nothing?
        }
    }

    unsafe { SetupDiDestroyDeviceInfoList(dev_info) };

    output
}
