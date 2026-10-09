# Networking

```text
 wget  git  browse/web  pkg  host  ping  nc  httpd           user space
 huldra_user::http ── huldra-http (HTTP/1.1)   huldra-tls (TLS 1.3, X.509)
 socket(), connect(), send()…                  huldra-crypto (ciphers, keys)
──────────────── syscalls ───────────────────────────────────────────────────
 kernel/src/net/socket.rs            sockets are files: read/write/poll/close
 huldra-net  (libs/net)              ARP · IPv4 · ICMP · UDP · TCP · DHCP · DNS
 netd thread                         feeds frames and time to the stack
 kernel/src/net/e1000.rs             Intel 8254x: DMA rings, interrupts
```

## Contents

- [The TCP/IP stack](#the-tcpip-stack)
- [Configuration](#configuration)
- [HTTP and HTTPS](#http-and-https)
- [Certificates](#certificates)
- [Randomness](#randomness)
- [Tools](#tools)
- [Limits](#limits)

## The TCP/IP stack

[`libs/net`](../libs/net) is a complete TCP/IP stack **without any I/O**.
It is fed received frames and the current time, and it hands back frames
to send. Because of that, the same code runs in the kernel and in host unit
tests. The tests connect two stacks over a simulated wire that drops a
third of the packets and has a slow reader.

| module | |
|---|---|
| `wire.rs` | Ethernet, ARP, IPv4, ICMP, UDP and TCP formats and checksums |
| `tcp.rs` | connections: handshake, in-order data, go-back-N retransmission with exponential backoff, flow control by the peer's window, orderly close, TIME-WAIT |
| `stack.rs` | interfaces, ARP cache, routing, sockets (TCP listen/accept/connect, UDP), ICMP echo, loopback |
| `dhcp.rs` | DHCP client (DISCOVER/OFFER/REQUEST/ACK) |
| `dns.rs` | DNS queries and answers for the resolver |

Sockets use the Linux numbers and structures (`AF_INET`, `SOCK_STREAM`,
`sockaddr_in`…), so the C library's sockets and static glibc programs work
unchanged.

## Configuration

The `ip=` kernel parameter:

- `ip=dhcp` (default): QEMU's user network gives `10.0.2.15`, with the host
  at `10.0.2.2` and DNS at `10.0.2.3`;
- `ip=10.0.2.15/24,10.0.2.2,10.0.2.3`: static address, gateway, DNS;
- `ip=none`: loopback only.

Names are looked up in `/etc/hosts`, then through DNS.
`/proc/net/if`, `/proc/net/sockets` and `/proc/net/dns` show the state, and
`ifconfig` prints it. QEMU's user network reaches the internet through the
host, so `wget https://...` and `git clone https://github.com/...` work
from inside the system.

## HTTP and HTTPS

`huldra_user::http` is the client every program uses:

- **HTTP/1.1** messages from [`libs/http`](../libs/http): request
  formatting, and an incremental response parser that handles
  `Content-Length`, chunked transfer coding and bodies that end at close.
  Bodies are streamed, so `wget` writes a download straight to the file.
- **Redirects** (301/302/303/307/308, `http → https` included), relative
  `Location`s resolved per RFC 3986.
- **gzip** response bodies (via [`libs/flate`](../libs/flate)).
- **TLS 1.3** ([`libs/tls`](../libs/tls)):
  - key exchange with X25519;
  - records encrypted with ChaCha20-Poly1305, or AES-128-GCM if the server
    prefers it;
  - the server proves its identity with ECDSA (P-256, P-384) or RSA-PSS.
    Its certificate chain is checked as described below;
  - SNI, ALPN `http/1.1`, KeyUpdate and alerts are handled.

All the cryptography is in [`libs/crypto`](../libs/crypto) (SHA-1/2,
HMAC, HKDF, ChaCha20-Poly1305, AES-GCM, X25519, RSA, ECDSA), written from
the specifications. It is tested against vectors from an independent
implementation (OpenSSL through Python's `cryptography`), including
rejection of tampered data. The TLS client itself runs against real
servers both in QEMU and on the host (`cargo run -p huldra-tls --example
fetch -- github.com`).

## Certificates

A server's certificate must:

- name the host, in its subjectAltName (DNS names, including `*.`
  wildcards for one label, or an IP address);
- be valid now (check `date`: the clock comes from the RTC);
- chain up to a trusted root through intermediates the server sends, each
  marked as a CA and each signature verified. SHA-1 signatures are
  rejected.

Trusted roots are `/etc/ssl/certs/ca-certificates.crt` plus any other
`*.pem` in `/etc/ssl/certs`. `cargo xtask build` copies the build machine's
bundle there: Mozilla's set, as Linux distributions and Git for Windows
ship it. Set `HULDRA_CA_BUNDLE` to choose another file. `--insecure`
(wget), `-k` (browse) and `GIT_SSL_NO_VERIFY=1` (git) skip the checks.

## Randomness

TLS keys need unpredictable numbers. `/dev/urandom`, `/dev/random` and
`getrandom` come from the kernel's generator (`kernel/src/random.rs`):
ChaCha20 with fast key erasure. It is seeded from RDSEED/RDRAND when the
CPU has them, from the jitter of the time-stamp counter and from the clock,
and it keeps mixing in the arrival times of interrupts.

## Tools

| | |
|---|---|
| `ifconfig` | interfaces and addresses |
| `ping HOST` | ICMP echo |
| `host NAME` | DNS lookup |
| `wget URL...` | downloads over HTTP and HTTPS: `-O`, `-P`, `-c` (resume with Range), `-q`, `-S` (headers), `--header`, `--post-data`, `--post-file`, `--compressed`, `--insecure`; a progress bar with speed and time left |
| `nc [-l] HOST PORT` | TCP client and server |
| `httpd [-p port] [dir]` | HTTP/1.0 file server; `cargo xtask run` forwards host `localhost:8080` to guest port 80 |
| `git` | clone, fetch, pull and push over smart HTTP(S); see [git](git.md) |
| `browse`, `web` | web browsers; see [browser](browser.md) |
| `pkg` | packages over HTTP(S); see [packages](packages.md) |

## Limits

- IPv4 only.
- TCP has no congestion control, and segments that arrive out of order
  are dropped (retransmission recovers them).
- TLS 1.3 only. A server that only speaks TLS 1.2 answers with a
  "handshake failure" alert, and so does one that requires a key exchange
  group other than X25519. Client certificates and session resumption are not
  supported.
