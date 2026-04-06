// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

//! Extra parameters beyond the request must implement SystemParam.
//! A plain `u32` does not, so the hidden IntoHandler assertion must fail.

#[galeon_engine::command]
pub struct MyRequest {
    pub id: u64,
}

pub struct MyResponse {
    pub ok: bool,
}

#[galeon_engine::handler]
pub fn bad_handler(cmd: MyRequest, _factor: u32) -> Result<MyResponse, String> {
    Ok(MyResponse { ok: true })
}

fn main() {}
