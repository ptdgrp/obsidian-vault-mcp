/// Timx8 - 8位时间有序短ID
///
/// 格式：7位秒级时间戳（2000-2100） + 1位毫秒区间（把1秒分成32份）
/// 字符集：小写 Crockford Base32（小写字母+数字，无符号）
const ENCODING: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// 2000-01-01 00:00:00 UTC
const EPOCH_2000: u64 = 946_684_800;

/// 生成一个小写 timx8 ID。
pub fn generate() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards");

    let secs = duration.as_secs();
    let millis = duration.subsec_millis(); // 0..999

    // 从 2000 年开始的秒数
    let seconds_since_2000 = secs.saturating_sub(EPOCH_2000);

    // 把 1000ms 平均分成 32 个区间（0~31）
    let slot = ((millis as u64) * 32 / 1000) as u8;

    encode(seconds_since_2000, slot)
}

/// 编码成 8 位字符串
fn encode(seconds: u64, slot: u8) -> String {
    let mut buf = [0u8; 8];

    // 最后 1 位：毫秒区间
    buf[7] = ENCODING[(slot & 0x1F) as usize];

    // 前 7 位：秒级时间戳
    let mut value = seconds;
    for i in (0..7).rev() {
        buf[i] = ENCODING[(value & 0x1F) as usize];
        value >>= 5;
    }

    // SAFETY: ENCODING 全是合法 ASCII
    String::from_utf8(buf.to_vec()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let secs = 1784444372u64;
        let millis = 293u64;
        let seconds_since_2000 = secs.saturating_sub(EPOCH_2000);
        let slot = ((millis) * 32 / 1000) as u8;
        assert_eq!(encode(seconds_since_2000, slot), "0ryycjm9");
    }
}
