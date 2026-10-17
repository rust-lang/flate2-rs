// Malformed-input acceptance and resetting unfinished streams differ in other backends.
#![cfg(all(feature = "fdeflate", not(feature = "any_zlib")))]

#[path = "../fuzz/streaming.rs"]
mod streaming;

#[test]
fn deterministic_chunk_flush_reset_and_corruption_matrix() {
    let mut state = 179u32;
    for size in [0, 1, 258, 1023, 1024, 32768, 98304, 131072, 500000] {
        for pattern in 0..4 {
            for level in 0..10 {
                let mut input: Vec<_> = (0..size + 8)
                    .map(|i| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        if i < 8 || pattern == 0 {
                            state as u8
                        } else if pattern == 1 {
                            b'a'
                        } else if pattern == 2 {
                            (state % 32) as u8
                        } else {
                            (i % 251) as u8
                        }
                    })
                    .collect();
                input[1] = level;
                streaming::exercise(&input);
            }
        }
    }
}
