# Reverse proxy deployment

The recommended production layout keeps the Rust server bound to loopback and terminates HTTPS in a reverse proxy.

```text
Internet / LAN
  -> HTTPS reverse proxy
     -> 127.0.0.1:8787
        -> SOOP Rust Web
```

Keep the application on `127.0.0.1:8787` unless you have a specific reason to expose the Axum listener directly.

## Caddy example

```caddyfile
soop.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:8787
}
```

Caddy can obtain and renew public TLS certificates automatically when DNS and inbound ports are configured correctly.

## Nginx example

```nginx
server {
    listen 443 ssl http2;
    server_name soop.example.com;

    ssl_certificate     /path/to/fullchain.pem;
    ssl_certificate_key /path/to/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_read_timeout 3600s;
    }
}
```

## Access-control notes

- The Web API requires the management bearer token, but HTTPS is still required when traffic leaves the local machine.
- Do not place the management token in a public repository, reverse-proxy config committed to Git, screenshots, or logs.
- Prefer VPN/LAN-only exposure if public internet access is not needed.
- If exposing publicly, restrict source networks at the reverse proxy/firewall when practical.
- Do not proxy unrelated local applications through the SOOP management hostname.

## Windows firewall

If Caddy/Nginx runs on the same Windows host, the Rust server itself can remain loopback-only and does not need an inbound firewall rule for port 8787. Only the reverse proxy's HTTPS port needs to be reachable.

## Health check

After deployment, open the HTTPS hostname in a browser and verify that the UI loads. The API status endpoint still requires the management token; a bare unauthenticated request returning HTTP 401 is expected and confirms the backend is not exposing management data anonymously.
