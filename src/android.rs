/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use crate::mrp;
use jni::objects::{JObject, JString, JValue};
use jni::sys::jobject;
use jni::JNIEnv;
use std::path::Path;

#[no_mangle]
pub extern "system" fn Java_org_skymrp_android_NativeMrpMetadataReader_nativeRead(
    mut env: JNIEnv<'_>,
    _reader: JObject<'_>,
    path: JString<'_>,
) -> jobject {
    read_metadata(&mut env, path).unwrap_or(std::ptr::null_mut())
}

fn read_metadata(env: &mut JNIEnv<'_>, path: JString<'_>) -> Result<jobject, String> {
    let path: String = env
        .get_string(&path)
        .map_err(|error| error.to_string())?
        .into();
    let metadata = mrp::read_metadata_from_file(Path::new(&path))?;
    let name = JObject::from(
        env.new_string(metadata.name)
            .map_err(|error| error.to_string())?,
    );
    let vendor = JObject::from(
        env.new_string(metadata.vendor)
            .map_err(|error| error.to_string())?,
    );
    let description = JObject::from(
        env.new_string(metadata.description)
            .map_err(|error| error.to_string())?,
    );
    let object = env
        .new_object(
            "org/skymrp/android/MrpMetadata",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;J)V",
            &[
                JValue::Object(&name),
                JValue::Object(&vendor),
                JValue::Object(&description),
                JValue::Long(i64::from(metadata.version)),
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(object.into_raw())
}
