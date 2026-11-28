//! 哈希/摘要:文本、HMAC、大文件流式。一次算齐 MD5 / SHA-1 / SHA-256 / SHA-512。
//! 纯计算无 UI——工具箱与 CLI 共用。

use digest::{Digest, KeyInit, Mac};
use hmac::Hmac;
use md5::Md5;
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::io::Read;
use std::path::Path;

/// 摘要算法。`Algo::ALL` 的顺序即 `Digests` 数组的下标
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algo {
    pub const ALL: [Algo; 4] = [Algo::Md5, Algo::Sha1, Algo::Sha256, Algo::Sha512];

    pub fn name(self) -> &'static str {
        match self {
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA-1",
            Algo::Sha256 => "SHA-256",
            Algo::Sha512 => "SHA-512",
        }
    }
}

/// 四种摘要的小写十六进制,顺序同 `Algo::ALL`
pub type Digests = [String; 4];

#[derive(Debug, PartialEq, Eq)]
pub struct FileDigests {
    pub size: u64,
    pub digests: Digests,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HashError {
    /// 进度回调后调用方置了取消标志
    Cancelled,
    Io(String),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashError::Cancelled => write!(f, "已取消"),
            HashError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// 文件分块大小:1 MiB(四算法同时更新,块太大白占内存)
pub const FILE_CHUNK: usize = 1024 * 1024;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 一次性算齐四种摘要(文本/小数据)
pub fn digest_bytes(data: &[u8]) -> Digests {
    [
        hex(&Md5::digest(data)),
        hex(&Sha1::digest(data)),
        hex(&Sha256::digest(data)),
        hex(&Sha512::digest(data)),
    ]
}

/// HMAC 原始摘要字节。密钥长度不限:超块长先散列、过短补零(RFC 2104)
pub fn hmac_bytes(algo: Algo, key: &[u8], data: &[u8]) -> Vec<u8> {
    // RFC 2104 对密钥长度没有限制,new_from_slice 永不返回 Err
    macro_rules! run {
        ($d:ty) => {{
            let mut mac = <Hmac<$d> as KeyInit>::new_from_slice(key)
                .expect("HMAC 密钥长度不受限");
            Mac::update(&mut mac, data);
            Mac::finalize(mac).into_bytes().to_vec()
        }};
    }
    match algo {
        Algo::Md5 => run!(Md5),
        Algo::Sha1 => run!(Sha1),
        Algo::Sha256 => run!(Sha256),
        Algo::Sha512 => run!(Sha512),
    }
}

/// HMAC,小写十六进制
pub fn hmac(algo: Algo, key: &[u8], data: &[u8]) -> String {
    hex(&hmac_bytes(algo, key, data))
}

/// 四算法同时推进:文件每读一块只喂一次
struct FileHashers {
    md5: Md5,
    sha1: Sha1,
    sha256: Sha256,
    sha512: Sha512,
}

impl FileHashers {
    fn update(&mut self, data: &[u8]) {
        Digest::update(&mut self.md5, data);
        Digest::update(&mut self.sha1, data);
        Digest::update(&mut self.sha256, data);
        Digest::update(&mut self.sha512, data);
    }

    fn finish(self) -> Digests {
        let Self {
            md5,
            sha1,
            sha256,
            sha512,
        } = self;
        [
            hex(&md5.finalize()),
            hex(&sha1.finalize()),
            hex(&sha256.finalize()),
            hex(&sha512.finalize()),
        ]
    }
}

/// 流式文件哈希:按 `chunk` 分块读,每读完一块调一次 `on_progress`(累计字节数)。
/// 取消检查在每次读取之前——取消后不再碰文件
pub fn digest_file(
    path: &Path,
    chunk: usize,
    cancelled: impl Fn() -> bool,
    mut on_progress: impl FnMut(u64),
) -> Result<FileDigests, HashError> {
    let mut f = std::fs::File::open(path)
        .map_err(|e| HashError::Io(format!("{}: {e}", path.display())))?;
    let mut buf = vec![0u8; chunk.max(1)];
    let mut hs = FileHashers {
        md5: Md5::new(),
        sha1: Sha1::new(),
        sha256: Sha256::new(),
        sha512: Sha512::new(),
    };
    let mut done = 0u64;
    loop {
        if cancelled() {
            return Err(HashError::Cancelled);
        }
        let n = f
            .read(&mut buf)
            .map_err(|e| HashError::Io(format!("{}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hs.update(&buf[..n]);
        done += n as u64;
        on_progress(done);
    }
    Ok(FileDigests {
        size: done,
        digests: hs.finish(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOX: &[u8] = b"The quick brown fox jumps over the lazy dog";

    /// 基准值取自系统 python3 hashlib(2026-11-28 生成),不是本实现的回声
    const V_EMPTY: [&str; 4] = [
        "d41d8cd98f00b204e9800998ecf8427e",
        "da39a3ee5e6b4b0d3255bfef95601890afd80709",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
    ];
    const V_ABC: [&str; 4] = [
        "900150983cd24fb0d6963f7d28e17f72",
        "a9993e364706816aba3e25717850c26c9cd0d89d",
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
    ];
    const V_FOX: [&str; 4] = [
        "9e107d9d372bb6826bd81d3542a419d6",
        "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12",
        "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592",
        "07e547d9586f6a73f73fbac0435ed76951218fb7d0c8d788a309d785436bbb642e93a252a954f23912547d1e8a3b5ed6e1bfd7097821233fa0538f3db854fee6",
    ];
    /// 255313 字节 `(i * 7 + i / 251) as u8` 的流式结果(独立于一次性计算的基准)
    const V_FILE: [&str; 4] = [
        "723563d11217165e2421d578ebe21e07",
        "8a64d9a650e07de6575937233e0879e615f5e3d3",
        "9d30c9101a143d53804ac5bbc6ef08aac76f7cc75103a3029aaa848adecbc929",
        "fdefe4daf1062ee2e65f82a0f45dfe64d3e1bda82edb4a3f9c14532133d39acc04ee46c86158dc4363d5e5029018651ee2991f6459686e739a007e0a10a5a3e1",
    ];

    fn strs(v: [&str; 4]) -> Digests {
        std::array::from_fn(|i| v[i].to_string())
    }

    /// 每个测试独占的临时目录(按 `tag` 区分,测试并行不冲突)
    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("shujing_hash_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn digest_matches_hashlib() {
        assert_eq!(digest_bytes(b""), strs(V_EMPTY));
        assert_eq!(digest_bytes(b"abc"), strs(V_ABC));
        assert_eq!(digest_bytes(FOX), strs(V_FOX));
    }

    #[test]
    fn hmac_matches_hashlib() {
        // 超块长密钥(120 字节)+ 含全部字节值的二进制消息
        let long_key: Vec<u8> = b"0123456789".iter().cycle().take(120).copied().collect();
        let bin: Vec<u8> = (0..=255u8).cycle().take(768).collect();
        // (key, msg, [MD5, SHA-1, SHA-256, SHA-512])
        let cases: [(&[u8], &[u8], [&str; 4]); 5] = [
            (
                b"",
                b"",
                [
                    "74e6f7298a9c2d168935f58c001bad88",
                    "fbdb1d1b18aa6c08324b7d64b71fb76370690e1d",
                    "b613679a0814d9ec772f95d778c35fc5ff1697c493715653c6c712144292c5ad",
                    "b936cee86c9f87aa5d3c6f2e84cb5a4239a5fe50480a6ec66b70ab5b1f4ac6730c6c515421b327ec1d69402e53dfb49ad7381eb067b338fd7b0cb22247225d47",
                ],
            ),
            (
                b"",
                FOX,
                [
                    "ad262969c53bc16032f160081c4a07a0",
                    "2ba7f707ad5f187c412de3106583c3111d668de8",
                    "fb011e6154a19b9a4c767373c305275a5a69e8b68b0b4c9200c383dced19a416",
                    "1de78322e11d7f8f1035c12740f2b902353f6f4ac4233ae455baccdf9f37791566e790d5c7682aad5d3ceca2feff4d3f3fdfd9a140c82a66324e9442b8af71b6",
                ],
            ),
            (
                b"key",
                b"",
                [
                    "63530468a04e386459855da0063b6596",
                    "f42bb0eeb018ebbd4597ae7213711ec60760843f",
                    "5d5d139563c95b5967b9bd9a8c9b233a9dedb45072794cd232dc1b74832607d0",
                    "84fa5aa0279bbc473267d05a53ea03310a987cecc4c1535ff29b6d76b8f1444a728df3aadb89d4a9a6709e1998f373566e8f824a8ca93b1821f0b69bc2a2f65e",
                ],
            ),
            (
                b"key",
                FOX,
                [
                    "80070713463e7749b90c2dc24911e275",
                    "de7c9b85b8b78aa6bc8a7a36f70a90701c9db4d9",
                    "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
                    "b42af09057bac1e2d41708e48a902e09b5ff7f12ab428a4fe86653c73dd248fb82f948a549f7b791a5b41915ee4d1ec3935357e4e2317250d0372afa2ebeeb3a",
                ],
            ),
            (
                long_key.as_slice(),
                bin.as_slice(),
                [
                    "fe9c053c0e8c78c42f364717c361e711",
                    "8e1323d6a91505bb7336f7249914b3ebb65ff80a",
                    "5087553662157ca4d774a61cc12a6c63eb3d141fa6b29bb95087821299e6b784",
                    "8e590f43578cb12013c4d25fa02c89f21a84c46b4e204781ceca5ce0adb73cbe463fa3548061f4b77ba98190b27a399b6a550b041e912da70559b6521093d59f",
                ],
            ),
        ];
        for (key, msg, want) in cases {
            for (algo, want) in Algo::ALL.into_iter().zip(want) {
                assert_eq!(
                    hmac(algo, key, msg),
                    want,
                    "{algo:?} key={}B msg={}B",
                    key.len(),
                    msg.len()
                );
            }
        }
    }

    #[test]
    fn hmac_output_lengths() {
        for (algo, len) in Algo::ALL.into_iter().zip([16usize, 20, 32, 64]) {
            assert_eq!(hmac_bytes(algo, b"k", b"m").len(), len, "{algo:?}");
        }
    }

    #[test]
    fn file_streaming_matches_hashlib() {
        let dir = tmp_dir("stream");
        let path = dir.join("bin.dat");
        let data: Vec<u8> = (0..255_313u32).map(|i| (i * 7 + i / 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let mut calls = 0usize;
        let mut last = 0u64;
        let r = digest_file(&path, 4093, || false, |done| {
            assert!(done > last, "进度应单调递增");
            last = done;
            calls += 1;
        })
        .unwrap();
        assert_eq!(r.size, data.len() as u64);
        assert_eq!(r.digests, strs(V_FILE));
        assert_eq!(r.digests, digest_bytes(&data), "流式应与一次性计算一致");
        assert_eq!(calls, 63, "255313 字节按 4093 分块");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_file_matches_empty_digest() {
        let dir = tmp_dir("empty");
        let path = dir.join("empty.dat");
        std::fs::write(&path, b"").unwrap();
        let mut calls = 0;
        let r = digest_file(&path, FILE_CHUNK, || false, |_| calls += 1).unwrap();
        assert_eq!(r.size, 0);
        assert_eq!(r.digests, strs(V_EMPTY));
        assert_eq!(calls, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_stops_after_current_chunk() {
        let dir = tmp_dir("cancel");
        let path = dir.join("cancel.dat");
        std::fs::write(&path, vec![7u8; 20_000]).unwrap();
        let stop = std::cell::Cell::new(false);
        let mut calls = 0;
        let err = digest_file(&path, 4093, || stop.get(), |_| {
            calls += 1;
            stop.set(true);
        })
        .unwrap_err();
        assert_eq!(err, HashError::Cancelled);
        assert_eq!(calls, 1, "取消后不该再读下一块");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_io_error() {
        let dir = tmp_dir("missing");
        let path = dir.join("nope.bin");
        let err = digest_file(&path, 1024, || false, |_| {}).unwrap_err();
        assert!(
            matches!(&err, HashError::Io(m) if m.contains("nope.bin")),
            "{err:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
