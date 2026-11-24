import test from "node:test";
import assert from "node:assert/strict";
import {
  acceleratorsFor,
  colabPillSummary,
  formatAcceleratorName,
  formatTierLabel,
  renderColabModalHtml,
} from "../../ui/colab.js";

test("formatTierLabel and formatAcceleratorName handle Ultra and TPU variants", () => {
  assert.equal(formatTierLabel("SUBSCRIPTION_TIER_PRO_PLUS"), "Pro+ / Ultra");
  assert.equal(formatTierLabel("SUBSCRIPTION_TIER_PRO"), "Pro");
  assert.equal(formatAcceleratorName("V5E1"), "v5e-1");
  assert.equal(formatAcceleratorName("V6E1"), "v6e-1");
  assert.equal(formatAcceleratorName("A100"), "A100");
});

test("colabPillSummary prioritizes Colab runtime over Container and Local", () => {
  const local = colabPillSummary(null, "");
  assert.equal(local.label, "Local");
  assert.equal(local.isColab, false);

  const container = colabPillSummary(null, "Container");
  assert.equal(container.label, "Container");
  assert.equal(container.isContainer, true);

  const colab = colabPillSummary(
    {
      hasOauthConfig: true,
      signedIn: true,
      email: "user@example.com",
      tier: "SUBSCRIPTION_TIER_PRO_PLUS",
      ccuBalance: 420.5,
      ccuRateHourly: 5.3,
      assignmentsCount: 1,
      specs: [],
      runtimes: [],
      activeRuntimeId: "rt-1",
      activeLabel: "Colab · TPU v5e-1",
      busy: "",
      error: "",
    },
    "Container",
  );
  assert.equal(colab.label, "Colab · TPU v5e-1");
  assert.equal(colab.isColab, true);
  assert.match(colab.title, /420\.5 CCU/);
});

test("acceleratorsFor prioritizes shape-specific eligibility and deduplicates specs", () => {
  const specs = [
    { variant: "VARIANT_GPU", accelerator: "T4", shape: "SHAPE_STANDARD", eligible: true },
    { variant: "VARIANT_GPU", accelerator: "A100", shape: "SHAPE_STANDARD", eligible: false },
    { variant: "VARIANT_GPU", accelerator: "A100", shape: "SHAPE_HIGHMEM", eligible: true },
    { variant: "VARIANT_TPU", accelerator: "V5E1", shape: "SHAPE_STANDARD", eligible: true },
  ];
  const gpuHigh = acceleratorsFor(specs, "VARIANT_GPU", "SHAPE_HIGHMEM");
  assert.deepEqual(gpuHigh.map((a) => [a.accelerator, a.eligible]), [
    ["T4", true],
    ["A100", true],
  ]);
  const tpu = acceleratorsFor(specs, "VARIANT_TPU", "SHAPE_STANDARD");
  assert.deepEqual(tpu.map((a) => a.label), ["v5e-1"]);
});

test("renderColabModalHtml renders OAuth setup and signed-in Ultra TPU controls safely", () => {
  const signedOutHtml = renderColabModalHtml(
    {
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
    },
    {
      clientId: "",
      clientSecret: "",
      oauthJson: "",
      directToken: "",
      tokenOpen: false,
      jsonOpen: false,
      variant: "VARIANT_GPU",
      accelerator: "A100",
      shape: "SHAPE_STANDARD",
    },
  );
  assert.match(signedOutHtml, /Google OAuth 2\.0 Sign-In/);
  assert.match(signedOutHtml, /Sign in with Google/);
  assert.match(signedOutHtml, /Direct Access Token/);

  const signedInHtml = renderColabModalHtml(
    {
      hasOauthConfig: true,
      signedIn: true,
      email: "user@example.com",
      tier: "SUBSCRIPTION_TIER_PRO_PLUS",
      ccuBalance: 450.25,
      ccuRateHourly: 2.1,
      assignmentsCount: 1,
      specs: [
        { variant: "VARIANT_TPU", accelerator: "V5E1", shape: "SHAPE_STANDARD", eligible: true },
        { variant: "VARIANT_TPU", accelerator: "V6E1", shape: "SHAPE_STANDARD", eligible: true },
      ],
      runtimes: [
        {
          runtimeId: "rt-99",
          variant: "VARIANT_TPU",
          accelerator: "V5E1",
          shape: "SHAPE_STANDARD",
          label: "Colab · TPU v5e-1",
          active: true,
        },
      ],
      activeRuntimeId: "rt-99",
      activeLabel: "Colab · TPU v5e-1",
      busy: "",
      error: "",
    },
    {
      clientId: "",
      clientSecret: "",
      oauthJson: "",
      directToken: "",
      tokenOpen: false,
      jsonOpen: false,
      variant: "VARIANT_TPU",
      accelerator: "V5E1",
      shape: "SHAPE_STANDARD",
    },
  );
  assert.match(signedInHtml, /Pro\+ \/ Ultra/);
  assert.match(signedInHtml, /450\.2 CCU|450\.3 CCU/);
  assert.match(signedInHtml, /v5e-1/);
  assert.match(signedInHtml, /v6e-1/);
  assert.match(signedInHtml, /Disconnect &amp; Terminate VM/);
});
