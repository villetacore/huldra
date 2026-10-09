"""Regenerates the certificates the tests use (Python `cryptography`):

    python libs/tls/testdata/generate.py

root.der      a CA (EC P-256), the trust anchor of the tests
inter.der     an intermediate CA (RSA 2048) signed by root
leaf.der      test.huldra and *.wild.huldra, RSA-PSS-free RSA 2048, by inter
expired.der   like leaf but expired in 2021
other.der     a leaf from an unknown CA
p384.der      a leaf with a P-384 key signed by root with ECDSA-SHA384

The QEMU test server (xtask) uses server.pem / server.key: a chain
leaf + inter for 10.0.2.2 and test.huldra.
"""
import datetime, os
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa

d = os.path.dirname(os.path.abspath(__file__))
now = datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)
far = datetime.datetime(2099, 1, 1, tzinfo=datetime.timezone.utc)

def name(cn): return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])

def cert(subject, key, issuer, issuer_key, ca, start=now, end=far, sans=(), ips=(), h=hashes.SHA256()):
    b = (x509.CertificateBuilder().subject_name(name(subject)).issuer_name(name(issuer))
         .public_key(key.public_key()).serial_number(x509.random_serial_number())
         .not_valid_before(start).not_valid_after(end)
         .add_extension(x509.BasicConstraints(ca=ca, path_length=None), critical=True))
    alt = [x509.DNSName(s) for s in sans] + [x509.IPAddress(__import__("ipaddress").ip_address(i)) for i in ips]
    if alt:
        b = b.add_extension(x509.SubjectAlternativeName(alt), critical=False)
    return b.sign(issuer_key, h)

def save(n, c): open(os.path.join(d, n), "wb").write(c.public_bytes(serialization.Encoding.DER))

root_key = ec.generate_private_key(ec.SECP256R1())
root = cert("Huldra Test Root", root_key, "Huldra Test Root", root_key, True)
inter_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
inter = cert("Huldra Test Intermediate", inter_key, "Huldra Test Root", root_key, True)
leaf_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
leaf = cert("test.huldra", leaf_key, "Huldra Test Intermediate", inter_key, False, sans=["test.huldra", "*.wild.huldra"], ips=["10.0.2.2"])
expired = cert("test.huldra", leaf_key, "Huldra Test Intermediate", inter_key, False,
               start=datetime.datetime(2020, 1, 1, tzinfo=datetime.timezone.utc), end=datetime.datetime(2021, 1, 1, tzinfo=datetime.timezone.utc), sans=["test.huldra"])
other_key = ec.generate_private_key(ec.SECP256R1())
other = cert("test.huldra", leaf_key, "Unknown CA", other_key, False, sans=["test.huldra"])
p384_key = ec.generate_private_key(ec.SECP384R1())
p384 = cert("p384.huldra", p384_key, "Huldra Test Root", root_key, False, sans=["p384.huldra"], h=hashes.SHA384())

for n, c in [("root.der", root), ("inter.der", inter), ("leaf.der", leaf), ("expired.der", expired), ("other.der", other), ("p384.der", p384)]:
    save(n, c)
open(os.path.join(d, "root.pem"), "wb").write(root.public_bytes(serialization.Encoding.PEM))
open(os.path.join(d, "server.pem"), "wb").write(leaf.public_bytes(serialization.Encoding.PEM) + inter.public_bytes(serialization.Encoding.PEM))
open(os.path.join(d, "server.key"), "wb").write(leaf_key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
print("ok")
