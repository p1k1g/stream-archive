# Reverse proxy deployment

Keep the Stream Archive Rust server on loopback and expose only Caddy or another reverse proxy over HTTPS.

```text
Internet
  -> router TCP 80/443
     -> PC LAN IPv4:80/443
        -> Caddy
           -> 127.0.0.1:8787
              -> Stream Archive
```

## Important: 127.0.0.1 is not a port-forward target

Do **not** configure a router rule such as `external 8787 -> 127.0.0.1:8787`.
`127.0.0.1` is loopback on the device that receives the packet. On a router it means the router itself, not the Windows PC.

Use the Windows PC's LAN IPv4 address for router forwarding, for example `192.168.0.35`:

```text
TCP 80  -> 192.168.0.35:80
TCP 443 -> 192.168.0.35:443
```

Keep Stream Archive itself at the default `127.0.0.1:8787`. There is normally no reason to expose port 8787 directly.

Find the PC address with:

```powershell
ipconfig
```

Reserve that address in the router's DHCP settings if possible so the forwarding rule does not break after a lease change.

## Caddy

The portable package contains `Caddyfile.example`. Copy or rename it to `Caddyfile` and replace the placeholder hostname:

```caddyfile
YOUR_DOMAIN.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:8787

    header {
        X-Content-Type-Options "nosniff"
        Referrer-Policy "strict-origin-when-cross-origin"
    }
}
```

Caddy automatically obtains and renews a public TLS certificate when:

- the hostname resolves to your public IP;
- inbound TCP 80/443 reach the Windows PC;
- the ISP/router is not blocking those ports;
- the connection is not behind unsupported CGNAT.

Start Caddy from the folder containing `caddy.exe` and `Caddyfile`:

```powershell
.\caddy.exe validate --config .\Caddyfile
.\caddy.exe run --config .\Caddyfile
```

Keep `stream-archive-server.exe` running separately. Neither process is registered as an OS service by this project.

## Windows firewall

If Caddy runs on the same Windows host, port 8787 can remain loopback-only and does not need an inbound firewall rule. Allow Caddy or TCP 80/443 instead. Example from an elevated PowerShell:

```powershell
New-NetFirewallRule -DisplayName "Stream Archive Caddy HTTP"  -Direction Inbound -Protocol TCP -LocalPort 80  -Action Allow
New-NetFirewallRule -DisplayName "Stream Archive Caddy HTTPS" -Direction Inbound -Protocol TCP -LocalPort 443 -Action Allow
```

## External testing

Some routers do not support NAT loopback/hairpin NAT. A public hostname can therefore fail while tested from the same home Wi-Fi even though it works externally. Test with a phone using Wi-Fi off and LTE/5G on.

If the router WAN address is in a private/CGNAT range such as `10.x.x.x`, `172.16-31.x.x`, `192.168.x.x`, or `100.64.0.0/10`, ordinary IPv4 port forwarding may not work. In that case request a public IPv4 address or use a VPN/tunnel approach instead.

## Direct LAN listener (not preferred for internet exposure)

For LAN-only troubleshooting you can bind Axum to all interfaces:

```powershell
$env:STREAM_ARCHIVE_BIND="0.0.0.0:8787"
.\stream-archive-server.exe
```

Then connect to the **PC LAN IPv4**, never `127.0.0.1`. For public internet use, switch back to `127.0.0.1:8787` and use an HTTPS reverse proxy.

## Security notes

- The Web API requires application authentication/recovery-token authorization, but HTTPS is still required when traffic leaves the local machine.
- Do not put the recovery token in the Caddyfile, Git, screenshots, or logs.
- Prefer VPN/LAN-only exposure if public internet access is unnecessary.
- If exposing publicly, source-IP restrictions or an additional authentication layer at the proxy can provide another defense layer.

## Health check

Open the HTTPS hostname and verify the UI loads. A bare unauthenticated management API request returning HTTP 401 is expected and confirms management data is not exposed anonymously.
