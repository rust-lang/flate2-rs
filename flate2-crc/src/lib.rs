// Note that this isn't really intended to be a user-facing crate, that's
// `flate2::Crc`

#[macro_use]
extern crate cfg_if;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

cfg_if! {
    if #[cfg(not(simd))] {
        mod other;
        use self::other as imp;
    } else if #[cfg(any(target_arch = "x86", target_arch = "x86_64"))] {
        mod x86;
        use self::x86 as imp;
    } else {
        mod other;
        use self::other as imp;
    }
}

#[derive(Debug)]
pub struct Hardware(bool);

impl Hardware {
    #[inline]
    pub fn detect() -> Hardware {
        Hardware(imp::detect())
    }

    #[inline]
    pub fn calculate(
        &self,
        crc: u32,
        data: &[u8],
        fallback: fn(u32, &[u8]) -> u32,
    ) -> u32 {
        if self.0 {
            unsafe { imp::calculate(crc, data, fallback) }
        } else {
            fallback(crc, data)
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate miniz_sys;
    extern crate rand;
    extern crate rayon;

    use self::rand::Rng;
    use self::rayon::prelude::*;
    use super::Hardware;

    fn fallback(a: u32, b: &[u8]) -> u32 {
        unsafe {
            miniz_sys::mz_crc32(a as _, b.as_ptr(), b.len()) as u32
        }
    }

    fn random_chunks(iters: usize, lo: usize, hi: usize) {
        let hardware = Hardware::detect();

        (0..iters)
            .into_par_iter()
            .for_each_with(Vec::new(), |data, _| {
                let mut rng = rand::thread_rng();
                let init = rng.gen::<u32>();
                let len = rng.gen_range(lo, hi);
                data.resize(len, 0u8);
                rng.fill(&mut data[..]);

                assert_eq!(
                    fallback(init, &data),
                    hardware.calculate(init, &data, fallback),
                );
            });
    }

    #[test]
    fn random_small() {
        random_chunks(1000, 0, 256);
    }

    #[test]
    fn random_med() {
        random_chunks(1000, 256, 16 * 1024);
    }

    #[test]
    fn random_large() {
        random_chunks(1000, 0, 1024 * 1024);
    }

    quickcheck! {
        fn prop(crc: u32, xs: Vec<u8>) -> bool {
            fallback(crc, &xs) == Hardware::detect().calculate(crc, &xs, fallback)
        }
    }
}
