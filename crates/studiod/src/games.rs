use anyhow::Result;
use studio_server::SummarizeRequest;

use crate::m4::Emitter;

pub fn summarize(_em: &Emitter, _req: &SummarizeRequest, _seq: &mut usize) -> Result<()> {
    Ok(())
}
