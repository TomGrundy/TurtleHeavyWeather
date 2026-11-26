/**
 * Constant-time string comparison to avoid timing leaks on TRIGGER_SECRET.
 */
function timingSafeEqual(a, b) {
  const enc = new TextEncoder();
  const ab = enc.encode(a);
  const bb = enc.encode(b);
  if (ab.length !== bb.length) {
    return false;
  }
  let diff = 0;
  for (let i = 0; i < ab.length; i++) {
    diff |= ab[i] ^ bb[i];
  }
  return diff === 0;
}

/**
 * Triggers GitHub Actions `workflow_dispatch` for the configured workflow.
 */
async function dispatchWorkflow(env, triggerLabel) {
  if (!env.GITHUB_TOKEN) {
    throw new Error("Missing GITHUB_TOKEN secret");
  }

  const ref = env.GITHUB_REF || "main";
  const url = `https://api.github.com/repos/${env.GITHUB_OWNER}/${env.GITHUB_REPO}/actions/workflows/${env.GITHUB_WORKFLOW}/dispatches`;

  const response = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${env.GITHUB_TOKEN}`,
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      "User-Agent": "thw-cron-dispatcher",
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ ref }),
  });

  if (!response.ok) {
    const body = await response.text();
    throw new Error(
      `GitHub workflow_dispatch failed (${response.status}): ${body}`
    );
  }

  console.log(
    `Dispatched ${env.GITHUB_WORKFLOW} on ${env.GITHUB_OWNER}/${env.GITHUB_REPO}@${ref} (${triggerLabel})`
  );
}

/**
 * Cloudflare Worker Cron Trigger + secret-protected HTTP trigger that
 * reliably dispatches `.github/workflows/sync-and-deploy.yml`.
 */
export default {
  async scheduled(controller, env, ctx) {
    await dispatchWorkflow(env, `cron: ${controller.cron}`);
  },

  async fetch(request, env, ctx) {
    if (request.method !== "POST") {
      return new Response("Method Not Allowed", { status: 405 });
    }
    if (!env.TRIGGER_SECRET) {
      return new Response("TRIGGER_SECRET not configured", { status: 503 });
    }

    const auth = request.headers.get("Authorization") || "";
    const token = auth.startsWith("Bearer ") ? auth.slice(7) : "";
    if (!token || !timingSafeEqual(token, env.TRIGGER_SECRET)) {
      return new Response("Unauthorized", { status: 401 });
    }

    try {
      await dispatchWorkflow(env, "manual HTTP POST");
      return Response.json(
        {
          ok: true,
          workflow: env.GITHUB_WORKFLOW,
          ref: env.GITHUB_REF || "main",
        },
        { status: 202 }
      );
    } catch (err) {
      return Response.json(
        { ok: false, error: err instanceof Error ? err.message : String(err) },
        { status: 502 }
      );
    }
  },
};
