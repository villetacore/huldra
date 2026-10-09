"""Regenerates vectors.txt with the Python `cryptography` package.

    python libs/crypto/testdata/generate.py > libs/crypto/testdata/vectors.txt

Every line is `kind field field ...` with fields in hex; tests.rs checks
Huldra's implementation against them.
"""
import hashlib, hmac, os, random
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa, ec, padding, x25519
from cryptography.hazmat.primitives.asymmetric.utils import decode_dss_signature
from cryptography.hazmat.primitives.ciphers.aead import ChaCha20Poly1305, AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF, HKDFExpand

random.seed(1)
def rnd(n): return bytes(random.getrandbits(8) for _ in range(n))
def h(b): return b.hex() if b else "-"
out = []
def line(*f): out.append(" ".join(f))

msgs = [b"", b"abc", rnd(55), rnd(56), rnd(64), rnd(111), rnd(112), rnd(128), rnd(1000), rnd(5000)]
for m in msgs:
    for name in ("sha1", "sha256", "sha384", "sha512"):
        line(name, h(m), hashlib.new(name, m).hexdigest())

for klen in (0, 20, 64, 100, 200):
    k, m = rnd(klen), rnd(77)
    line("hmac-sha256", h(k), h(m), hmac.new(k, m, "sha256").hexdigest())
    line("hmac-sha384", h(k), h(m), hmac.new(k, m, "sha384").hexdigest())
for n in (16, 32, 42, 100):
    salt, ikm, info = rnd(13), rnd(22), rnd(10)
    okm = HKDF(algorithm=hashes.SHA256(), length=n, salt=salt, info=info).derive(ikm)
    line("hkdf-sha256", h(salt), h(ikm), h(info), str(n), h(okm))

for n in (0, 1, 15, 16, 17, 63, 64, 65, 1000, 3000):
    key, nonce, aad, pt = rnd(32), rnd(12), rnd(n % 29), rnd(n)
    line("chacha20poly1305", h(key), h(nonce), h(aad), h(pt), h(ChaCha20Poly1305(key).encrypt(nonce, pt, aad)))
    key = rnd(16)
    line("aes128gcm", h(key), h(nonce), h(aad), h(pt), h(AESGCM(key).encrypt(nonce, pt, aad)))

for _ in range(4):
    a, b = x25519.X25519PrivateKey.generate(), x25519.X25519PrivateKey.generate()
    raw = lambda k: k.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw, serialization.NoEncryption())
    pub = lambda k: k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    line("x25519", h(raw(a)), h(pub(a)), h(pub(b)), h(a.exchange(b.public_key())))

hash_of = {"sha1": hashes.SHA1(), "sha256": hashes.SHA256(), "sha384": hashes.SHA384(), "sha512": hashes.SHA512()}
for bits in (2048, 3072, 4096):
    key = rsa.generate_private_key(public_exponent=65537, key_size=bits)
    nums = key.public_key().public_numbers()
    n, e = nums.n.to_bytes(bits // 8, "big"), nums.e.to_bytes(3, "big")
    for hn in ("sha1", "sha256", "sha384", "sha512"):
        m = rnd(50)
        line("rsa-pkcs1", hn, h(n), h(e), h(m), h(key.sign(m, padding.PKCS1v15(), hash_of[hn])))
    for hn in ("sha256", "sha384", "sha512"):
        m = rnd(50)
        sig = key.sign(m, padding.PSS(mgf=padding.MGF1(hash_of[hn]), salt_length=hash_of[hn].digest_size), hash_of[hn])
        line("rsa-pss", hn, h(n), h(e), h(m), h(sig))

for curve, size, hn in ((ec.SECP256R1(), 32, "sha256"), (ec.SECP384R1(), 48, "sha384"), (ec.SECP256R1(), 32, "sha384"), (ec.SECP384R1(), 48, "sha256")):
    for _ in range(3):
        key = ec.generate_private_key(curve)
        point = key.public_key().public_bytes(serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)
        m = rnd(40)
        r, s = decode_dss_signature(key.sign(m, ec.ECDSA(hash_of[hn])))
        line("ecdsa", "p256" if size == 32 else "p384", hn, h(point), h(m), h(r.to_bytes(size, "big")), h(s.to_bytes(size, "big")))

print("\n".join(out))
