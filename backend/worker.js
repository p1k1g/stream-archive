const SOOP_API = "https://live.sooplive.com/afreeca/player_live_api.php";

function json(data, status = 200) {
  return new Response(JSON.stringify(data, null, 2), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

function authorized(request, env) {
  const supplied = request.headers.get("x-api-key") || "";
  return Boolean(env.API_SECRET) && supplied === env.API_SECRET;
}

async function postForm(url, data, headers = {}) {
  const body = new URLSearchParams();

  for (const [key, value] of Object.entries(data)) {
    body.set(key, value ?? "");
  }

  return fetch(url, {
    method: "POST",
    redirect: "follow",
    headers: {
      "content-type": "application/x-www-form-urlencoded; charset=UTF-8",
      "user-agent":
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36",
      ...headers,
    },
    body,
  });
}

async function health(request) {
  let outboundIp = null;

  try {
    const r = await fetch("https://api.ipify.org");
    if (r.ok) outboundIp = (await r.text()).trim();
  } catch {}

  return json({
    ok: true,
    outbound_ip: outboundIp,
    colo: request.cf?.colo ?? null,
    client_country: request.cf?.country ?? null,
  });
}

async function getSoopUrl(request) {
  let input;

  try {
    input = await request.json();
  } catch {
    return json({ success: false, error: "invalid_json" }, 400);
  }

  const account = String(input.account || input.bid || "");
  const bno = String(input.bno || "");
  const rmd = String(input.rmd || "");

  // Always request the SOOP master HLS playlist.
  // Streamlink on the local PC chooses best/1080p/etc from this master.
  const quality = String(input.quality || "master");
  const cq = String(input.cq || "sd");

  const password = String(
    input.password ||
    input.bpwd ||
    "",
  );

  // FIX12 PowerShell sends "cookie".
  // Keep legacy aliases for compatibility with previous watcher builds.
  const cookie = String(
    input.cookie ||
    input.soop_cookie_header ||
    "",
  );

  if (!account || !bno || !rmd) {
    return json(
      {
        success: false,
        error: "account_bno_rmd_required",
      },
      400,
    );
  }

  const commonHeaders = {
    referer: `https://play.sooplive.com/${account}`,
    origin: "https://play.sooplive.com",
  };

  if (cookie) {
    commonHeaders.cookie = cookie;
  }

  // 1) Request AID from the Cloudflare Worker location.
  const aidRes = await postForm(
    SOOP_API,
    {
      from_api: "0",
      mode: "landing",
      player_type: "html5",
      stream_type: "common",
      type: "aid",
      bid: account,
      bno,
      pwd: password,
      quality,
      cq,
    },
    commonHeaders,
  );

  const aidText = await aidRes.text();

  if (!aidRes.ok) {
    return json(
      {
        success: false,
        stage: "aid_http",
        status: aidRes.status,
        error: aidText.slice(0, 300),
      },
      502,
    );
  }

  let aidJson;

  try {
    aidJson = JSON.parse(aidText);
  } catch {
    return json(
      {
        success: false,
        stage: "aid_json",
        error: aidText.slice(0, 300),
      },
      502,
    );
  }

  const channel = aidJson?.CHANNEL;

  if (
    !channel ||
    Number(channel.RESULT) !== 1 ||
    !String(channel.AID || "").trim()
  ) {
    return json(
      {
        success: false,
        stage: "aid",
        result: channel?.RESULT ?? null,
      },
      502,
    );
  }

  const aid = String(channel.AID);

  // Prefer global CDN return types.
  const cdns = ["gcp_cdn", "azure_cdn", "aws_cf"];
  const assignBase = `${rmd.replace(/\/$/, "")}/broad_stream_assign.html`;

  // Master playlist is intentional.
  const broadKey = `${bno}-common-${quality}-hls`;
  const errors = [];

  for (const cdn of cdns) {
    try {
      const assign = new URL(assignBase);
      assign.searchParams.set("return_type", cdn);
      assign.searchParams.set("broad_key", broadKey);

      const assignHeaders = {
        referer: `https://play.sooplive.com/${account}`,
        origin: "https://play.sooplive.com",
        "user-agent":
          "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/151 Safari/537.36",
      };

      // Usually AID is the important auth token at this step, but forwarding
      // the same SOOP auth cookie is harmless and helps login-restricted cases.
      if (cookie) {
        assignHeaders.cookie = cookie;
      }

      const assignRes = await fetch(assign.toString(), {
        redirect: "follow",
        headers: assignHeaders,
      });

      const text = await assignRes.text();

      if (!assignRes.ok) {
        errors.push(
          `${cdn}: HTTP ${assignRes.status} ${text.slice(0, 120)}`.trim(),
        );
        continue;
      }

      let obj;

      try {
        obj = JSON.parse(text);
      } catch {
        errors.push(`${cdn}: invalid JSON ${text.slice(0, 120)}`.trim());
        continue;
      }

      if (!obj.view_url) {
        errors.push(`${cdn}: no view_url`);
        continue;
      }

      const view = new URL(String(obj.view_url));

      if (!view.hostname.includes("live-global-cdn")) {
        errors.push(`${cdn}: non-global host ${view.hostname}`);
        continue;
      }

      view.searchParams.set("aid", aid);

      return json({
        success: true,
        quality,
        cdn,
        host: view.hostname,
        playlist_url: view.toString(),
      });
    } catch (error) {
      errors.push(`${cdn}: ${String(error?.message || error)}`);
    }
  }

  return json(
    {
      success: false,
      stage: "assign",
      broad_key: broadKey,
      error: errors.join(" || "),
    },
    502,
  );
}

export default {
  async fetch(request, env) {
    if (!authorized(request, env)) {
      return new Response("Unauthorized", { status: 401 });
    }

    const url = new URL(request.url);

    if (request.method === "GET" && url.pathname === "/health") {
      return health(request);
    }

    if (request.method === "POST" && url.pathname === "/soop/url") {
      return getSoopUrl(request);
    }

    return new Response("Not Found", { status: 404 });
  },
};
