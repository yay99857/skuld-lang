---
title: Networking
description: Use blocking TCP, DNS and HTTP clients with explicit errors and TLS requirements.
status: Implemented
---
Networking APIs are synchronous and blocking. They do not introduce threads, async functions or an event loop.

## HTTP

`http.get(url: string) -> Result<Response, HttpError>` fetches a plain HTTP/1.1 response. `Response` exposes `status`, `reason`, `headers`, `body` and `header(name) -> Option<string>`.

```skuld main.skuld
import "std/http"

func main() {
    match http.get("http://127.0.0.1:8080/data.json") {
        Ok(response): print(response.body)
        Err(error): print(http.describe(error))
    }
}
```

This example needs a server on the stated loopback address. No server is started automatically.

## TCP and DNS

`net.open(address: string, port: int)` returns a `Result<Connection, NetError>`. Follow the connection's close discipline when using it directly. `dns.resolve(name)` resolves an IPv4 A record and returns a Result. There is no DNS cache.

## HTTPS

Use `std/https` for HTTPS and link OpenSSL yourself: `-lssl -lcrypto` on Linux, `-llibssl -llibcrypto` on Windows, where the MSVC toolchain uses those library names. TLS verifies the certificate and host name, and there is no insecure override. On Windows, certificates are checked against the machine's certificate store, so `SSL_CERT_FILE` is not needed. `std/http` itself refuses `https://` URLs.

```bash
skuld run fetch.skuld -lssl -lcrypto
```

## Converting errors

Each networking module has its own error type. A function that calls several of them converts at each call with `map_err`, as `std/http` does:

```skuld
let address = dns.resolve(host).map_err((reason) => HttpError.Name(reason))?
```

## Availability

The current source includes TCP, DNS, HTTP, TLS and HTTPS modules on Linux and Windows. HTTPS needs OpenSSL installed and linked, as described above.
