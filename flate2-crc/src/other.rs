#[inline]
pub fn detect() -> bool {
    false
}

pub unsafe fn calculate(
    _crc: u32,
    _data: &[u8],
    _fallback: fn(u32, &[u8]) -> u32,
) -> u32 {
    panic!()
}
