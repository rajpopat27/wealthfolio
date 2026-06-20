# Local Wealthfolio Connect

This mode points Wealthfolio at an external Connect-compatible broker service
instead of `api.wealthfolio.app`. The broker service is a separate process; the
Wealthfolio app only calls its HTTP API.

Start the broker service first:

```bash
cd /Users/raj.popat/investment-tracker/wealthfolio-connect-local
. .venv/bin/activate
wealthfolio-connect-local
```

Then start Wealthfolio with local Connect enabled. By default this uses
`http://127.0.0.1:8787`:

```bash
cd /Users/raj.popat/investment-tracker/wealthfolio-clean
pnpm dev:tauri:local-connect
```

To point Wealthfolio at any other broker service URL, pass the URL before the
script:

```bash
CONNECT_API_URL=https://broker.example.com \
CONNECT_AUTH_URL=https://broker.example.com \
pnpm dev:tauri:local-connect
```

The default local values are:

```text
CONNECT_API_URL=http://127.0.0.1:8787
CONNECT_AUTH_URL=http://127.0.0.1:8787
CONNECT_AUTH_PUBLISHABLE_KEY=local
CONNECT_LOCAL_MODE=true
```

In local mode the frontend skips Supabase login, stores a local refresh token,
and lets the existing broker sync commands pull connections, accounts, holdings,
and activities from the local service.

For broker callbacks, set the broker service's `WFC_PUBLIC_BASE_URL` to the URL
Kite/Fyers can redirect to. Configure broker developer consoles with:

```text
${WFC_PUBLIC_BASE_URL}/brokers/zerodha/callback
${WFC_PUBLIC_BASE_URL}/brokers/fyers/callback
```
