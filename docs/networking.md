# Networking

```text
 socket(), connect(), send()…        user space: wget, httpd, nc, ping, host, pkg
──────────────── syscalls ───────────────────────────────────────────────────
 kernel/src/net/socket.rs            sockets are files: read/write/poll/close
 huldra-net  (libs/net)              ARP · IPv4 · ICMP · UDP · TCP · DHCP · DNS
 netd thread                         feeds frames and time to the stack
 kernel/src/net/e1000.rs             Intel 82540EM: DMA rings, interrupts
```

## The stack

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

## Configuration

The `ip=` kernel parameter:

- `ip=dhcp` (default): QEMU's user network gives `10.0.2.15`, with the host
  at `10.0.2.2` and DNS at `10.0.2.3`;
- `ip=10.0.2.15/24,10.0.2.2,10.0.2.3`: static address, gateway, DNS;
- `ip=none`: loopback only.

`/proc/net/if`, `/proc/net/sockets` and `/proc/net/dns` show the state.
`ifconfig` prints it.

## Tools

| | |
|---|---|
| `ifconfig` | interfaces and addresses |
| `ping HOST` | ICMP echo |
| `host NAME` | DNS lookup |
| `wget URL` | HTTP download |
| `nc [-l] HOST PORT` | TCP client and server |
| `httpd [-p port] [dir]` | HTTP/1.0 file server; `cargo xtask run` forwards host `localhost:8080` to guest port 80 |
| `pkg` | uses the same HTTP client ([packages](packages.md)) |

Sockets use the Linux numbers and structures (`AF_INET`, `SOCK_STREAM`,
`sockaddr_in`…), so the C library's sockets and static glibc programs work
unchanged.

## Limits

IPv4 only. TCP has no congestion control and no reassembly of out-of-order
segments, which are dropped and recovered by retransmission.
