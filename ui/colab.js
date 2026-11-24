/* Google Colab GPU/TPU runtime selector & authentication modal for Data mode. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/ColabStatusView").ColabStatusView} ColabStatusView */
/** @typedef {import("./bindings/ColabRuntimeSpec").ColabRuntimeSpec} ColabRuntimeSpec */
/** @typedef {import("./protocol").Dispatch} Dispatch */

/**
 * Format subscription tier enum string for display.
 * @param {string | undefined} tier
 * @returns {string}
 */
export function formatTierLabel(tier) {
  if (!tier || tier === "SUBSCRIPTION_TIER_UNSPECIFIED") return "";
  if (tier === "SUBSCRIPTION_TIER_PRO_PLUS") return "Pro+ / Ultra";
  if (tier === "SUBSCRIPTION_TIER_PRO") return "Pro";
  if (tier === "SUBSCRIPTION_TIER_FREE") return "Free";
  return tier.replace(/^SUBSCRIPTION_TIER_/, "");
}

/**
 * Format accelerator identifier nicely (e.g. V5E1 -> v5e-1).
 * @param {string} acc
 * @returns {string}
 */
export function formatAcceleratorName(acc) {
  const up = (acc || "").toUpperCase();
  if (up === "V2_8" || up === "V28") return "v2-8";
  if (up === "V5E1" || up === "V5E_1") return "v5e-1";
  if (up === "V6E1" || up === "V6E_1") return "v6e-1";
  if (!up || up === "NONE") return "CPU";
  return up;
}

/**
 * Compute Data canvas-bar pill label, tone, and tooltip from Colab + notebook state.
 * @param {ColabStatusView | null | undefined} colab
 * @param {string | null | undefined} notebookKernelLabel
 * @returns {{ label: string, title: string, isColab: boolean, isContainer: boolean }}
 */
export function colabPillSummary(colab, notebookKernelLabel) {
  const active = colab?.activeLabel || (notebookKernelLabel?.startsWith("Colab") ? notebookKernelLabel : "");
  if (active) {
    const ccu = colab?.ccuBalance != null ? ` (${colab.ccuBalance.toFixed(1)} CCU)` : "";
    return {
      label: active,
      title: `Running on ${active}${ccu} — click to switch GPU/TPU/Local or terminate VM`,
      isColab: true,
      isContainer: false,
    };
  }
  if (notebookKernelLabel === "Container") {
    return {
      label: "Container",
      title: "Running inside attached dev container — click to switch to Google Colab GPU/TPU",
      isColab: false,
      isContainer: true,
    };
  }
  return {
    label: "Local",
    title: "Local Jupyter kernel — click to connect Google Colab GPU/TPU runtime (:colab)",
    isColab: false,
    isContainer: false,
  };
}

/**
 * Build candidate accelerator cards for the selected variant & shape.
 * @param {ColabRuntimeSpec[]} specs
 * @param {string} variant
 * @param {string} shape
 * @returns {Array<{ accelerator: string, label: string, eligible: boolean }>}
 */
export function acceleratorsFor(specs, variant, shape) {
  /** @type {Map<string, boolean>} */
  const fromApi = new Map();
  for (const spec of specs || []) {
    if (spec.variant !== variant) continue;
    const acc = (spec.accelerator || "NONE").toUpperCase();
    const prev = fromApi.get(acc);
    if (spec.shape === shape) {
      fromApi.set(acc, spec.eligible);
    } else if (prev === undefined) {
      fromApi.set(acc, spec.eligible);
    }
  }

  if (variant === "VARIANT_CPU") {
    const eligible = fromApi.get("NONE") ?? true;
    return [{ accelerator: "NONE", label: shape === "SHAPE_HIGHMEM" ? "High-RAM CPU" : "Standard CPU", eligible }];
  }

  /** @type {string[]} */
  const preferred = variant === "VARIANT_TPU"
    ? ["V2_8", "V5E1", "V6E1"]
    : ["T4", "L4", "A100", "H100"];

  if (fromApi.size === 0) {
    return preferred.map((acc) => ({
      accelerator: acc,
      label: formatAcceleratorName(acc),
      eligible: true,
    }));
  }

  const ordered = [];
  const seen = new Set();
  for (const acc of preferred) {
    if (fromApi.has(acc)) {
      ordered.push({ accelerator: acc, label: formatAcceleratorName(acc), eligible: Boolean(fromApi.get(acc)) });
      seen.add(acc);
    }
  }
  for (const [acc, eligible] of fromApi.entries()) {
    if (!seen.has(acc) && acc !== "NONE") {
      ordered.push({ accelerator: acc, label: formatAcceleratorName(acc), eligible });
    }
  }
  return ordered;
}

/**
 * Render `#colab-modal` body HTML.
 * @param {ColabStatusView} status
 * @param {{
 *   clientId: string,
 *   clientSecret: string,
 *   oauthJson: string,
 *   directToken: string,
 *   tokenOpen: boolean,
 *   jsonOpen: boolean,
 *   variant: string,
 *   accelerator: string,
 *   shape: string,
 * }} draft
 * @returns {string}
 */
export function renderColabModalHtml(status, draft) {
  const tierLabel = formatTierLabel(status.tier);
  const subParts = [];
  if (status.activeLabel) subParts.push(status.activeLabel);
  else if (status.signedIn && status.email) subParts.push(status.email);
  else subParts.push("Connect TPU or GPU compute credits to Data mode notebooks");

  const errorBanner = status.error
    ? `<div class="cb-banner cb-err">${esc(status.error)}</div>`
    : "";
  const busyBanner = status.busy
    ? `<div class="cb-banner cb-busy"><span class="cb-spin"></span>${esc(status.busy)}</div>`
    : "";

  let body = "";
  if (!status.signedIn) {
    const busyDis = status.busy ? " disabled" : "";
    const oauthReadyChip = status.hasOauthConfig
      ? `<span class="chip ok">OAuth configured</span>`
      : `<span class="chip">OAuth setup needed</span>`;
    body = `
      <div class="cb-sec cb-card">
        <div class="cb-row-head">
          <strong>Google OAuth 2.0 Sign-In (Desktop App)</strong>
          ${oauthReadyChip}
        </div>
        <ol class="cb-steps">
          <li>Create an <strong>OAuth 2.0 Desktop App</strong> client in Google Cloud Console (enable <code>colaboratory</code> scope / add yourself as a Test User if Testing).</li>
          <li>Enter your Client ID &amp; Secret below, or paste the downloaded <code>client_secret.json</code>.</li>
          <li>Click <strong>Sign in with Google</strong> to authorize via localhost browser redirect.</li>
        </ol>
        <div class="cb-fields">
          <label class="st-field">Client ID
            <input type="text" data-colab-client-id value="${esc(draft.clientId)}" placeholder="123456-abc.apps.googleusercontent.com" spellcheck="false">
          </label>
          <label class="st-field">Client Secret
            <input type="password" data-colab-client-secret value="${esc(draft.clientSecret)}" placeholder="GOCSPX-..." spellcheck="false">
          </label>
        </div>
        <details class="cb-details"${draft.jsonOpen ? " open" : ""} data-colab-json-details>
          <summary>Or paste downloaded <code>client_secret_*.json</code></summary>
          <div class="cb-details-body">
            <textarea class="cb-textarea" data-colab-oauth-json rows="3" spellcheck="false" placeholder='{"installed":{"client_id":"...","client_secret":"..."}}'>${esc(draft.oauthJson)}</textarea>
            <div class="cb-actions">
              <button class="btn" data-colab-import-json${busyDis}>Import JSON</button>
            </div>
          </div>
        </details>
        <div class="cb-actions">
          <button class="btn" data-colab-save-oauth${busyDis}>Save OAuth</button>
          <button class="btn go" data-colab-signin${busyDis}>Sign in with Google</button>
        </div>
      </div>

      <details class="cb-sec cb-card cb-details"${draft.tokenOpen ? " open" : ""} data-colab-token-details>
        <summary>Direct Access Token (<code>gcloud</code> / manual fallback)</summary>
        <div class="cb-details-body">
          <p class="rm-muted">Run <code>gcloud auth application-default login --scopes=openid,email,profile,https://www.googleapis.com/auth/colaboratory &amp;&amp; gcloud auth application-default print-access-token</code> and paste below, or copy one directly:</p>
          <div class="cb-token-row">
            <input type="password" data-colab-token value="${esc(draft.directToken)}" placeholder="ya29...." spellcheck="false">
            <button class="btn" type="button" data-colab-gcloud-token${busyDis} title="Run gcloud auth application-default print-access-token | pbcopy">Copy gcloud Token</button>
            <button class="btn" data-colab-use-token${busyDis}>Use Token</button>
          </div>
        </div>
      </details>
    `;
  } else {
    const busyDis = status.busy ? " disabled" : "";
    const ccuText = status.ccuBalance != null
      ? `${status.ccuBalance.toFixed(1)} CCU`
      : "CCU unavailable";
    const rateText = status.ccuRateHourly != null && status.ccuRateHourly > 0
      ? ` (${status.ccuRateHourly.toFixed(2)}/hr)`
      : "";

    const activeList = status.runtimes.length
      ? `
        <div class="cb-sec">
          <div class="cb-label">Active Colab Runtimes (${status.runtimes.length})</div>
          <div class="cb-runtimes">
            ${status.runtimes.map((rt) => `
              <div class="cb-rt${rt.active ? " on" : ""}">
                <span class="cb-rt-name">${esc(rt.label)}</span>
                <span class="chip">${esc(rt.runtimeId)}</span>
                ${rt.active ? `<span class="chip ok">Attached</span>` : `<button class="btn" data-colab-attach="${esc(rt.runtimeId)}"${busyDis}>Attach</button>`}
                <button class="btn danger" data-colab-terminate="${esc(rt.runtimeId)}"${busyDis} title="Stop VM &amp; release GPU/TPU credits">Terminate</button>
              </div>
            `).join("")}
          </div>
        </div>
      `
      : "";

    const accels = acceleratorsFor(status.specs, draft.variant, draft.shape);
    const chosenAcc = accels.some((a) => a.accelerator === draft.accelerator)
      ? draft.accelerator
      : (accels.find((a) => a.eligible)?.accelerator ?? accels[0]?.accelerator ?? "NONE");

    body = `
      <div class="cb-sec cb-account">
        <div class="cb-acct-info">
          <strong>${esc(status.email || "Google Account")}</strong>
          ${tierLabel ? `<span class="chip accent">${esc(tierLabel)}</span>` : ""}
          <span class="chip info" title="Compute Units remaining">${esc(ccuText + rateText)}</span>
        </div>
        <div class="cb-actions">
          <button class="btn" type="button" data-colab-gcloud-token${busyDis} title="Run gcloud auth application-default print-access-token | pbcopy and refresh token">Copy gcloud Token</button>
          <button class="btn" data-colab-refresh${busyDis}>Refresh</button>
          <button class="btn" data-colab-signout${busyDis}>Sign Out</button>
        </div>
      </div>

      ${activeList}

      <div class="cb-sec cb-card">
        <div class="cb-row-head">
          <strong>Provision Compute Runtime</strong>
          <div class="seg" role="group" aria-label="Memory shape">
            <button data-colab-shape="SHAPE_STANDARD" class="${draft.shape === "SHAPE_STANDARD" ? "on" : ""}">Standard</button>
            <button data-colab-shape="SHAPE_HIGHMEM" class="${draft.shape === "SHAPE_HIGHMEM" ? "on" : ""}">High-RAM</button>
          </div>
        </div>
        <div class="seg cb-variants" role="group" aria-label="Hardware variant">
          <button data-colab-variant="VARIANT_GPU" class="${draft.variant === "VARIANT_GPU" ? "on" : ""}">GPU</button>
          <button data-colab-variant="VARIANT_TPU" class="${draft.variant === "VARIANT_TPU" ? "on" : ""}">TPU</button>
          <button data-colab-variant="VARIANT_CPU" class="${draft.variant === "VARIANT_CPU" ? "on" : ""}">CPU</button>
        </div>
        <div class="cb-accels">
          ${accels.map((a) => `
            <button class="cb-accel${a.accelerator === chosenAcc ? " on" : ""}${a.eligible ? "" : " dim"}"
                    data-colab-accel="${esc(a.accelerator)}"
                    ${a.eligible ? "" : 'title="Not marked eligible on your current Colab tier/quota"'}
                    ${busyDis}>
              <span class="cb-accel-name">${esc(a.label)}</span>
              ${a.eligible ? "" : `<span class="cb-accel-tag">ineligible</span>`}
            </button>
          `).join("")}
        </div>
        <div class="cb-actions cb-connect-row">
          <button class="btn go" data-colab-connect="${esc(chosenAcc)}"${busyDis}>Connect Runtime</button>
        </div>
      </div>
    `;
  }

  const footerActions = status.activeRuntimeId
    ? `
      <button class="btn" data-colab-local${status.busy ? " disabled" : ""}>Use Local Kernel</button>
      <button class="btn danger" data-colab-terminate-active${status.busy ? " disabled" : ""}>Disconnect &amp; Terminate VM</button>
    `
    : `<span class="rm-muted">Kernels auto-route here when a Colab runtime is attached.</span>`;

  return `
    <header class="rm-head">
      <div class="rm-brand">${icon("cloud")} <strong>GOOGLE COLAB</strong></div>
      <div class="rm-sub">${esc(subParts.join(" · "))}</div>
      <button class="btn" data-colab-close>Close ⎋</button>
    </header>
    <div class="rm-body">
      ${errorBanner}
      ${busyBanner}
      ${body}
      <div class="rm-footer">
        <div class="cb-actions">${footerActions}</div>
      </div>
    </div>
  `;
}

/**
 * Wire Colab modal interactions and live runtime pill updates.
 * @param {{
 *   root: Document,
 *   openOverlay: () => Promise<void>,
 *   closeOverlay: () => Promise<void> | void,
 *   dispatch: Dispatch,
 *   colabGcloudToken?: () => Promise<string>,
 *   flashMessage?: (text: string, level?: string) => void,
 * }} opts
 */
export function createColab({ root, openOverlay, closeOverlay, dispatch, colabGcloudToken, flashMessage }) {
  /** @type {ColabStatusView} */
  let status = {
    hasOauthConfig: false,
    signedIn: false,
    email: "",
    tier: "",
    ccuBalance: null,
    ccuRateHourly: null,
    assignmentsCount: 0,
    specs: [],
    runtimes: [],
    activeRuntimeId: "",
    activeLabel: "",
    busy: "",
    error: "",
  };
  /** @type {string} */
  let notebookKernelLabel = "";

  const draft = {
    clientId: "",
    clientSecret: "",
    oauthJson: "",
    directToken: "",
    tokenOpen: false,
    jsonOpen: false,
    variant: "VARIANT_GPU",
    accelerator: "A100",
    shape: "SHAPE_STANDARD",
  };

  function modalEl() {
    return root.getElementById("colab-modal");
  }

  function updatePill() {
    const pill = /** @type {HTMLElement | null} */ (root.querySelector("[data-nb-runtime]"));
    if (!pill) return;
    const summary = colabPillSummary(status, notebookKernelLabel);
    pill.textContent = summary.label;
    pill.title = summary.title;
    pill.classList.toggle("accent", summary.isColab);
    pill.classList.toggle("info", summary.isContainer);
  }

  function render() {
    updatePill();
    const box = modalEl();
    if (!box) return;
    const prevFocusAttr = captureFocusSelector(root.activeElement, box);
    box.innerHTML = renderColabModalHtml(status, draft);
    if (prevFocusAttr) {
      const target = /** @type {HTMLElement | null} */ (box.querySelector(prevFocusAttr));
      target?.focus?.({ preventScroll: true });
    }
  }

  function syncDraftFromDom() {
    const box = modalEl();
    if (!box) return;
    const cid = /** @type {HTMLInputElement | null} */ (box.querySelector("[data-colab-client-id]"));
    const csec = /** @type {HTMLInputElement | null} */ (box.querySelector("[data-colab-client-secret]"));
    const ojson = /** @type {HTMLTextAreaElement | null} */ (box.querySelector("[data-colab-oauth-json]"));
    const tok = /** @type {HTMLInputElement | null} */ (box.querySelector("[data-colab-token]"));
    const tokDet = /** @type {HTMLDetailsElement | null} */ (box.querySelector("[data-colab-token-details]"));
    const jsonDet = /** @type {HTMLDetailsElement | null} */ (box.querySelector("[data-colab-json-details]"));
    if (cid) draft.clientId = cid.value;
    if (csec) draft.clientSecret = csec.value;
    if (ojson) draft.oauthJson = ojson.value;
    if (tok) draft.directToken = tok.value;
    if (tokDet) draft.tokenOpen = tokDet.open;
    if (jsonDet) draft.jsonOpen = jsonDet.open;
  }

  root.addEventListener("input", (e) => {
    const t = /** @type {HTMLElement | null} */ (e.target);
    if (t?.closest?.("#colab-modal")) syncDraftFromDom();
  });

  root.addEventListener("toggle", (e) => {
    const t = /** @type {HTMLElement | null} */ (e.target);
    if (t?.closest?.("#colab-modal")) syncDraftFromDom();
  }, true);

  root.addEventListener("click", async (e) => {
    const target = /** @type {HTMLElement | null} */ (e.target);
    if (!target) return;

    if (target.closest("[data-nb-runtime]")) {
      await openModal();
      return;
    }

    const box = modalEl();
    if (!box || !box.contains(target)) return;
    syncDraftFromDom();

    if (target.closest("[data-colab-close]")) {
      await closeOverlay();
      return;
    }
    if (target.closest("[data-colab-save-oauth]")) {
      await dispatch({ type: "colabConfigureOauth", clientId: draft.clientId, clientSecret: draft.clientSecret });
      return;
    }
    if (target.closest("[data-colab-import-json]")) {
      await dispatch({ type: "colabImportOauthJson", json: draft.oauthJson });
      return;
    }
    if (target.closest("[data-colab-signin]")) {
      if (draft.clientId.trim()) {
        await dispatch({ type: "colabConfigureOauth", clientId: draft.clientId, clientSecret: draft.clientSecret });
      }
      await dispatch({ type: "colabSignIn" });
      return;
    }
    if (target.closest("[data-colab-gcloud-token]")) {
      if (!colabGcloudToken) {
        status = { ...status, error: "Fetching a gcloud token requires the Proton desktop app." };
        render();
        return;
      }
      status = { ...status, error: "", busy: "Fetching gcloud access token…" };
      render();
      try {
        const token = (await colabGcloudToken()).trim();
        draft.directToken = token;
        draft.tokenOpen = true;
        status = { ...status, busy: "" };
        flashMessage?.("Copied gcloud access token to clipboard", "info");
        if (status.signedIn) {
          render();
          await dispatch({ type: "colabSetToken", accessToken: token });
        } else {
          render();
        }
      } catch (err) {
        status = { ...status, busy: "", error: String(err) };
        flashMessage?.(String(err), "err");
        render();
      }
      return;
    }
    if (target.closest("[data-colab-use-token]")) {
      await dispatch({ type: "colabSetToken", accessToken: draft.directToken });
      return;
    }
    if (target.closest("[data-colab-refresh]")) {
      await dispatch({ type: "colabStatus" });
      return;
    }
    if (target.closest("[data-colab-signout]")) {
      await dispatch({ type: "colabSignOut" });
      return;
    }

    const variantBtn = /** @type {HTMLElement | null} */ (target.closest("[data-colab-variant]"));
    if (variantBtn) {
      draft.variant = variantBtn.dataset.colabVariant || "VARIANT_GPU";
      const list = acceleratorsFor(status.specs, draft.variant, draft.shape);
      draft.accelerator = list.find((a) => a.eligible)?.accelerator ?? list[0]?.accelerator ?? "NONE";
      render();
      return;
    }

    const shapeBtn = /** @type {HTMLElement | null} */ (target.closest("[data-colab-shape]"));
    if (shapeBtn) {
      draft.shape = shapeBtn.dataset.colabShape || "SHAPE_STANDARD";
      render();
      return;
    }

    const accelBtn = /** @type {HTMLElement | null} */ (target.closest("[data-colab-accel]"));
    if (accelBtn) {
      draft.accelerator = accelBtn.dataset.colabAccel || "NONE";
      render();
      return;
    }

    const connectBtn = /** @type {HTMLElement | null} */ (target.closest("[data-colab-connect]"));
    if (connectBtn) {
      const accel = connectBtn.dataset.colabConnect || draft.accelerator;
      await dispatch({ type: "colabConnect", variant: draft.variant, accelerator: accel, shape: draft.shape });
      return;
    }

    const attachBtn = /** @type {HTMLElement | null} */ (target.closest("[data-colab-attach]"));
    if (attachBtn?.dataset.colabAttach) {
      await dispatch({ type: "colabAttach", runtimeId: attachBtn.dataset.colabAttach });
      return;
    }

    if (target.closest("[data-colab-local]")) {
      await dispatch({ type: "colabDisconnect", unassign: false });
      return;
    }

    const termBtn = /** @type {HTMLElement | null} */ (target.closest("[data-colab-terminate]"));
    if (termBtn?.dataset.colabTerminate) {
      await dispatch({ type: "colabDisconnect", unassign: true });
      return;
    }

    if (target.closest("[data-colab-terminate-active]")) {
      await dispatch({ type: "colabDisconnect", unassign: true });
    }
  });

  async function openModal() {
    render();
    await openOverlay();
    await dispatch({ type: "colabStatus" });
  }

  /** @param {ColabStatusView} next */
  function onStatus(next) {
    status = next;
    render();
  }

  /** @param {string} label */
  function onNotebookLabel(label) {
    notebookKernelLabel = label || "";
    updatePill();
  }

  return { openModal, onStatus, onNotebookLabel };
}

/**
 * Remember an input attribute selector inside `#colab-modal` so background
 * status updates don't steal typing focus.
 * @param {Element | null} active
 * @param {HTMLElement} box
 * @returns {string | null}
 */
function captureFocusSelector(active, box) {
  if (!active || !box.contains(active)) return null;
  for (const attr of [
    "data-colab-client-id",
    "data-colab-client-secret",
    "data-colab-oauth-json",
    "data-colab-token",
  ]) {
    if (active.hasAttribute(attr)) return `[${attr}]`;
  }
  return null;
}
