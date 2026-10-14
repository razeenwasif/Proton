//! Colour as people see it, in whole numbers: OKLab (Björn Ottosson's), its
//! lightness even to the eye where sRGB's is not, and palette ramps made in
//! it — a colour's darker and lighter steps, the hue turning toward yellow as
//! it lightens and away as it darkens, as a painter's shading does.
//!
//! sRGB's curve and the sines are tables made once, outside the engine, and
//! checked against their formulas in floats by `tests/generate.rs`; the
//! matrices are Ottosson's in 1/65536. Cube roots are whole-number roots. So
//! a ramp is the same colours in every build.

/// sRGB's 8-bit values made linear, in 1/65535.
pub const LIN: [u16; 256] = [
    0, 20, 40, 60, 80, 99, 119, 139, 159, 179, 199, 219, 241, 264, 288, 313,
    340, 367, 396, 427, 458, 491, 526, 562, 599, 637, 677, 718, 761, 805, 851, 898,
    947, 997, 1048, 1101, 1156, 1212, 1270, 1330, 1391, 1453, 1517, 1583, 1651, 1720, 1790, 1863,
    1937, 2013, 2090, 2170, 2250, 2333, 2418, 2504, 2592, 2681, 2773, 2866, 2961, 3058, 3157, 3258,
    3360, 3464, 3570, 3678, 3788, 3900, 4014, 4129, 4247, 4366, 4488, 4611, 4736, 4864, 4993, 5124,
    5257, 5392, 5530, 5669, 5810, 5953, 6099, 6246, 6395, 6547, 6700, 6856, 7014, 7174, 7335, 7500,
    7666, 7834, 8004, 8177, 8352, 8528, 8708, 8889, 9072, 9258, 9445, 9635, 9828, 10022, 10219, 10417,
    10619, 10822, 11028, 11235, 11446, 11658, 11873, 12090, 12309, 12530, 12754, 12980, 13209, 13440, 13673, 13909,
    14146, 14387, 14629, 14874, 15122, 15371, 15623, 15878, 16135, 16394, 16656, 16920, 17187, 17456, 17727, 18001,
    18277, 18556, 18837, 19121, 19407, 19696, 19987, 20281, 20577, 20876, 21177, 21481, 21787, 22096, 22407, 22721,
    23038, 23357, 23678, 24002, 24329, 24658, 24990, 25325, 25662, 26001, 26344, 26688, 27036, 27386, 27739, 28094,
    28452, 28813, 29176, 29542, 29911, 30282, 30656, 31033, 31412, 31794, 32179, 32567, 32957, 33350, 33745, 34143,
    34544, 34948, 35355, 35764, 36176, 36591, 37008, 37429, 37852, 38278, 38706, 39138, 39572, 40009, 40449, 40891,
    41337, 41785, 42236, 42690, 43147, 43606, 44069, 44534, 45002, 45473, 45947, 46423, 46903, 47385, 47871, 48359,
    48850, 49344, 49841, 50341, 50844, 51349, 51858, 52369, 52884, 53401, 53921, 54445, 54971, 55500, 56032, 56567,
    57105, 57646, 58190, 58737, 59287, 59840, 60396, 60955, 61517, 62082, 62650, 63221, 63795, 64372, 64952, 65535,
];

/// A quarter of a sine wave, in 1/32767: `SIN[i]` is sin(i/256 · 90°).
const SIN: [u16; 257] = [
    0, 201, 402, 603, 804, 1005, 1206, 1407, 1608, 1809, 2009, 2210, 2410, 2611, 2811, 3012,
    3212, 3412, 3612, 3811, 4011, 4210, 4410, 4609, 4808, 5007, 5205, 5404, 5602, 5800, 5998, 6195,
    6393, 6590, 6786, 6983, 7179, 7375, 7571, 7767, 7962, 8157, 8351, 8545, 8739, 8933, 9126, 9319,
    9512, 9704, 9896, 10087, 10278, 10469, 10659, 10849, 11039, 11228, 11417, 11605, 11793, 11980, 12167, 12353,
    12539, 12725, 12910, 13094, 13279, 13462, 13645, 13828, 14010, 14191, 14372, 14553, 14732, 14912, 15090, 15269,
    15446, 15623, 15800, 15976, 16151, 16325, 16499, 16673, 16846, 17018, 17189, 17360, 17530, 17700, 17869, 18037,
    18204, 18371, 18537, 18703, 18868, 19032, 19195, 19357, 19519, 19680, 19841, 20000, 20159, 20317, 20475, 20631,
    20787, 20942, 21096, 21250, 21403, 21554, 21705, 21856, 22005, 22154, 22301, 22448, 22594, 22739, 22884, 23027,
    23170, 23311, 23452, 23592, 23731, 23870, 24007, 24143, 24279, 24413, 24547, 24680, 24811, 24942, 25072, 25201,
    25329, 25456, 25582, 25708, 25832, 25955, 26077, 26198, 26319, 26438, 26556, 26674, 26790, 26905, 27019, 27133,
    27245, 27356, 27466, 27575, 27683, 27790, 27896, 28001, 28105, 28208, 28310, 28411, 28510, 28609, 28706, 28803,
    28898, 28992, 29085, 29177, 29268, 29358, 29447, 29534, 29621, 29706, 29791, 29874, 29956, 30037, 30117, 30195,
    30273, 30349, 30424, 30498, 30571, 30643, 30714, 30783, 30852, 30919, 30985, 31050, 31113, 31176, 31237, 31297,
    31356, 31414, 31470, 31526, 31580, 31633, 31685, 31736, 31785, 31833, 31880, 31926, 31971, 32014, 32057, 32098,
    32137, 32176, 32213, 32250, 32285, 32318, 32351, 32382, 32412, 32441, 32469, 32495, 32521, 32545, 32567, 32589,
    32609, 32628, 32646, 32663, 32678, 32692, 32705, 32717, 32728, 32737, 32745, 32752, 32757, 32761, 32765, 32766,
    32767,
];

/// The sine of `angle` — 1024ths of a turn — in 1/32767.
pub fn sin(angle: i32) -> i64 {
    let a = angle.rem_euclid(1024) as usize;
    let (quarter, i) = (a / 256, a % 256);
    match quarter {
        0 => SIN[i] as i64,
        1 => SIN[256 - i] as i64,
        2 => -(SIN[i] as i64),
        _ => -(SIN[256 - i] as i64),
    }
}

/// The cosine of `angle`, 1024ths of a turn, in 1/32767.
pub fn cos(angle: i32) -> i64 {
    sin(angle + 256)
}

// Ottosson's matrices, in 1/65536: linear sRGB to LMS, LMS' to Lab, and back.
const M1: [[i64; 3]; 3] = [[27015, 35149, 3372], [13887, 44610, 7038], [5787, 18463, 41286]];
const M2: [[i64; 3]; 3] = [[13792, 52011, -267], [129630, -159160, 29530], [1698, 51300, -52997]];
const I2: [[i64; 3]; 3] = [[65536, 25974, 14143], [65536, -6918, -4185], [65536, -5864, -84639]];
const I1: [[i64; 3]; 3] = [[267173, -216774, 15137], [-83128, 171033, -22369], [-275, -46099, 111910]];

/// A colour in OKLab, in 1/65536: lightness from 0 to 65536, and a and b,
/// green to red and blue to yellow, within about ±26000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lab {
    pub l: i64,
    pub a: i64,
    pub b: i64,
}

fn times(m: &[[i64; 3]; 3], v: [i64; 3]) -> [i64; 3] {
    let row = |r: [i64; 3]| (r[0] * v[0] + r[1] * v[1] + r[2] * v[2]) >> 16;
    [row(m[0]), row(m[1]), row(m[2])]
}

/// The whole cube root of `n`, rounded down.
fn icbrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    // Roots of what is passed here are under 2¹⁷; their cubes fit.
    let (mut lo, mut hi) = (0i64, (1i64 << 17).min(n));
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if mid * mid * mid <= n {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// A straight sRGB colour in OKLab.
pub fn to_lab(c: [u8; 4]) -> Lab {
    let lms = times(&M1, [LIN[c[0] as usize] as i64, LIN[c[1] as usize] as i64, LIN[c[2] as usize] as i64]);
    // ∛(v / 65536) · 65536 = ∛(v · 65536²).
    let root = lms.map(|v| icbrt(v.max(0) << 32));
    let [l, a, b] = times(&M2, root);
    Lab { l, a, b }
}

/// The 8-bit sRGB value whose linear value is nearest `v` (1/65535).
fn encode(v: i64) -> u8 {
    let v = v.clamp(0, 65535) as u16;
    let at = LIN.partition_point(|&x| x < v);
    if at == 0 {
        return 0;
    }
    if at == 256 {
        return 255;
    }
    if v - LIN[at - 1] <= LIN[at] - v { (at - 1) as u8 } else { at as u8 }
}

/// An OKLab colour in sRGB — none if it is outside what sRGB can show, by
/// more than a hair.
pub fn from_lab(lab: Lab) -> Option<[u8; 3]> {
    let [l, m, s] = times(&I2, [lab.l, lab.a, lab.b]).map(|v| (((v * v) >> 16) * v) >> 16);
    let rgb = times(&I1, [l, m, s]);
    if rgb.iter().any(|&v| !(-64..=65535 + 64).contains(&v)) {
        return None;
    }
    Some(rgb.map(encode))
}

/// The ramp of a colour: `steps` colours, darkest first, from `spread`
/// below its lightness to `spread` above (1/65536, held to what can be
/// seen), the hue turning by up to `shift` degrees — toward yellow as the
/// ramp lightens, away from it as it darkens — and a little less chroma at
/// either end. Colours sRGB cannot show lose chroma until it can.
pub fn ramp(base: [u8; 4], steps: u32, shift: u32, spread: u32) -> Vec<[u8; 4]> {
    let steps = steps.clamp(1, 32);
    let lab = to_lab(base);
    if steps == 1 {
        return vec![[base[0], base[1], base[2], 255]];
    }
    let spread = spread.min(60000) as i64;
    let (lo, hi) = ((lab.l - spread).max(9000), (lab.l + spread).min(62000));
    let (lo, hi) = if hi - lo < 9000 { ((hi - 9000).max(0), (lo + 9000).min(65536)) } else { (lo, hi) };
    // Toward yellow (a little green of +b) is anticlockwise when the colour
    // is clockwise of it.
    let toward = if lab.a * 12000 - lab.b * (-1500) >= 0 { 1 } else { -1 };
    let turn = shift.min(180) as i64 * 1024 / 360;
    (0..steps as i64)
        .map(|i| {
            let t = i * 65536 / (steps as i64 - 1);
            let l = lo + (hi - lo) * t / 65536;
            let off = t - 32768;
            let angle = (toward * turn * off / 32768) as i32;
            let (c, s) = (cos(angle), sin(angle));
            let keep = 65536 - off.abs() * 6 / 10;
            let mut a = (((lab.a * c - lab.b * s) >> 15) * keep) >> 16;
            let mut b = (((lab.a * s + lab.b * c) >> 15) * keep) >> 16;
            for _ in 0..40 {
                if let Some(rgb) = from_lab(Lab { l, a, b }) {
                    return [rgb[0], rgb[1], rgb[2], 255];
                }
                a = a * 15 / 16;
                b = b * 15 / 16;
            }
            let grey = from_lab(Lab { l, a: 0, b: 0 }).unwrap_or([128, 128, 128]);
            [grey[0], grey[1], grey[2], 255]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_go_to_oklab_and_back() {
        for c in [[0, 0, 0, 255], [255, 255, 255, 255], [200, 30, 60, 255], [20, 160, 90, 255], [40, 60, 220, 255], [128, 128, 128, 255]] {
            let back = from_lab(to_lab(c)).unwrap();
            for i in 0..3 {
                assert!((back[i] as i32 - c[i] as i32).abs() <= 1, "{c:?} came back {back:?}");
            }
        }
        assert!(to_lab([255, 255, 255, 255]).l > 65000 && to_lab([0, 0, 0, 255]).l == 0);
        assert_eq!((sin(0), sin(256), sin(512), sin(768), cos(0)), (0, 32767, 0, -32767, 32767));
        assert_eq!(icbrt(27), 3);
        assert_eq!(icbrt(1 << 48), 1 << 16);
    }
}
