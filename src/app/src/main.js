function tauri() {
  if (!window.__TAURI__) {
    throw new Error("Tauri API not available. Run with: cd src/app && npm run dev");
  }
  return window.__TAURI__;
}

const projectInput = document.querySelector("#project-path");
const audioInput = document.querySelector("#audio-path");
const runButton = document.querySelector("#run-verify");
const progressPanel = document.querySelector("#progress-panel");
const progressFill = document.querySelector("#progress-fill");
const stageLabel = document.querySelector("#stage-label");
const progressPercent = document.querySelector("#progress-percent");
const progressPlan = document.querySelector("#progress-plan");
const progressDetail = document.querySelector("#progress-detail");
const resultPanel = document.querySelector("#result-panel");
const verdictBadge = document.querySelector("#verdict-badge");
const claimText = document.querySelector("#claim-text");
const coverageStat = document.querySelector("#coverage-stat");
const coverageDetail = document.querySelector("#coverage-detail");
const strongStat = document.querySelector("#strong-stat");
const possibleStat = document.querySelector("#possible-stat");
const bestMatch = document.querySelector("#best-match");
const errorText = document.querySelector("#error-text");
const openReportBtn = document.querySelector("#open-report");
const openFolderBtn = document.querySelector("#open-folder");
const publishPanel = document.querySelector("#publish-panel");
const artistInput = document.querySelector("#artist-name");
const songInput = document.querySelector("#song-title");
const publishBtn = document.querySelector("#publish-btn");
const publishStatus = document.querySelector("#publish-status");
const publishResult = document.querySelector("#publish-result");
const publishUrl = document.querySelector("#publish-url");
const openPageBtn = document.querySelector("#open-page");
const openQrBtn = document.querySelector("#open-qr");
const copyUrlBtn = document.querySelector("#copy-url");

let lastResult = null;
let lastPublish = null;
let running = false;
let publishing = false;
let estimatedSeconds = null;
let planSummaryLine = null;

const STAGE_LABELS = {
  "Scanning project": "Reading session",
  "Normalizing audio": "Preparing final track",
  "Analyzing fingerprints": "Scanning stems",
  "Matching recordings": "Matching stems",
  "Timeline coverage": "Checking final track",
  "Collecting evidence": "Building receipt",
  "Writing report": "Saving report",
  Complete: "Done",
  Working: "Working",
};

const VERDICT_LABELS = {
  PASS: "Yours",
  FAIL: "Doesn't match",
};

const CLAIM_LABELS = {
  PASS: "Your session stems explain this track. Save the report — for your bio, Bandcamp, or anyone who asks.",
  FAIL: "This bounce doesn't line up with stems in that project. Different mix, export, or wrong .band?",
};

function formatDuration(seconds) {
  const total = Math.max(0, Math.round(seconds));
  const m = Math.floor(total / 60);
  const s = total % 60;
  if (m > 0) {
    return `${m}:${String(s).padStart(2, "0")}`;
  }
  return `${total}s`;
}

function humanStage(label) {
  return STAGE_LABELS[label] || label;
}

function basename(path) {
  if (!path) return "";
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

function updateRunState() {
  runButton.disabled =
    running || !projectInput.value.trim() || !audioInput.value.trim();
}

function formatProgressLabel(percent) {
  const pct = Math.max(0, Math.min(1, percent)) * 100;
  if (estimatedSeconds && percent > 0 && percent < 1) {
    const remaining = Math.max(0, estimatedSeconds * (1 - percent));
    return `${pct.toFixed(1)}% · ~${Math.ceil(remaining)}s left`;
  }
  return `${pct.toFixed(1)}%`;
}

function setBar(fraction) {
  const clamped = Math.max(0, Math.min(1, fraction));
  progressFill.style.width = `${clamped * 100}%`;
  progressPercent.textContent = formatProgressLabel(clamped);
}

function resetProgress() {
  estimatedSeconds = null;
  planSummaryLine = null;
  setBar(0);
  stageLabel.textContent = "Starting";
  progressPlan.textContent = "";
  progressPlan.classList.add("hidden");
  progressDetail.textContent = "";
}

function formatPlanSummary(summary) {
  if (!summary) return "";
  return `${formatDuration(summary.song_seconds)} final track · ${summary.stem_count} stems in session · ~${Math.ceil(summary.estimated_seconds)}s`;
}

function handleProgress(evt) {
  if (evt.estimated_seconds != null) {
    estimatedSeconds = evt.estimated_seconds;
  }
  if (evt.plan_summary) {
    planSummaryLine = formatPlanSummary(evt.plan_summary);
  }

  const overall = evt.percent ?? 0;
  setBar(overall);
  stageLabel.textContent = humanStage(evt.stage_label || evt.message || "Working");

  if (planSummaryLine) {
    progressPlan.textContent = planSummaryLine;
    progressPlan.classList.remove("hidden");
  }

  const itemName = basename(evt.item_name);
  const itemPrefix =
    itemName && evt.item_total != null && evt.item_index != null
      ? `Stem ${evt.item_index + 1} of ${evt.item_total} · ${itemName}`
      : "";
  progressDetail.textContent = itemPrefix || evt.detail || "";
}

function showResultPanel() {
  resultPanel.classList.remove("hidden");
}

function hideResultPanel() {
  resultPanel.classList.add("hidden");
}

function showInlineError(message) {
  errorText.textContent = message;
  errorText.classList.remove("hidden");
  showResultPanel();
}

function updatePublishState() {
  const canPublish =
    !publishing &&
    lastResult?.verdict === "PASS" &&
    artistInput.value.trim() &&
    songInput.value.trim();
  publishBtn.disabled = !canPublish;
}

function hidePublishPanel() {
  publishPanel.classList.add("hidden");
  publishResult.classList.add("hidden");
  publishStatus.classList.add("hidden");
  publishStatus.textContent = "";
  lastPublish = null;
}

function showPublishPanel() {
  publishPanel.classList.remove("hidden");
  publishResult.classList.add("hidden");
  publishStatus.classList.add("hidden");
  updatePublishState();
}

function renderPublishResult(result) {
  lastPublish = result;
  publishStatus.classList.add("hidden");
  publishResult.classList.remove("hidden");
  publishUrl.textContent = result.url;
}

function renderResult(result) {
  lastResult = result;
  lastPublish = null;
  errorText.classList.add("hidden");
  errorText.textContent = "";

  verdictBadge.textContent =
    VERDICT_LABELS[result.verdict] || result.verdict;
  verdictBadge.className = `verdict ${result.verdict.toLowerCase()}`;

  claimText.textContent =
    CLAIM_LABELS[result.verdict] || result.claim;
  const pct = (result.coverage_ratio * 100).toFixed(1);
  coverageStat.textContent = `${pct}%`;
  if (result.timeline_target_seconds > 0) {
    coverageDetail.textContent = `${formatDuration(
      result.timeline_explained_seconds,
    )} of ${formatDuration(result.timeline_target_seconds)} of your track comes from session stems`;
  } else {
    coverageDetail.textContent = "How much of your track is explained by the project";
  }
  strongStat.textContent = String(result.strong_match_count);
  possibleStat.textContent = String(result.possible_match_count);

  if (result.best_match) {
    const b = result.best_match;
    bestMatch.textContent = `Strongest stem: ${basename(b.asset)} · ${formatDuration(
      b.offset_seconds,
    )}`;
  } else {
    bestMatch.textContent = "";
  }

  if (result.verdict === "PASS") {
    showPublishPanel();
  } else {
    hidePublishPanel();
  }

  showResultPanel();
}

function renderError(message) {
  lastResult = null;
  lastPublish = null;
  hidePublishPanel();
  verdictBadge.textContent = "Error";
  verdictBadge.className = "verdict error";
  claimText.textContent = "";
  coverageStat.textContent = "—";
  coverageDetail.textContent = "";
  strongStat.textContent = "—";
  possibleStat.textContent = "—";
  bestMatch.textContent = "";
  showInlineError(message);
}

function normalizeDialogPath(selected) {
  if (!selected) return null;
  return Array.isArray(selected) ? selected[0] : selected;
}

async function pickProject() {
  try {
    const path = normalizeDialogPath(
      await tauri().dialog.open({
        multiple: false,
        title: "Choose session file (.band)",
        filters: [{ name: "GarageBand session", extensions: ["band"] }],
      }),
    );
    if (path) {
      if (!path.toLowerCase().endsWith(".band")) {
        renderError("Choose a GarageBand .band session folder.");
        return;
      }
      projectInput.value = path;
      updateRunState();
    }
  } catch (err) {
    renderError(`Browse project failed: ${err}`);
  }
}

async function pickAudio() {
  try {
    const path = normalizeDialogPath(
      await tauri().dialog.open({
        multiple: false,
        title: "Choose final track (WAV or MP3)",
        filters: [
          {
            name: "Audio",
            extensions: ["wav", "mp3", "m4a", "aiff", "flac"],
          },
        ],
      }),
    );
    if (path) {
      audioInput.value = path;
      updateRunState();
    }
  } catch (err) {
    renderError(`Browse audio failed: ${err}`);
  }
}

async function runVerification() {
  if (running) return;
  running = true;
  hideResultPanel();
  progressPanel.classList.remove("hidden");
  resetProgress();
  runButton.disabled = true;

  try {
    const result = await tauri().core.invoke("run_verification", {
      project: projectInput.value,
      audio: audioInput.value,
    });
    handleProgress({
      percent: 1,
      stage_label: "Complete",
      message: "Done",
      step: "complete",
    });
    renderResult(result);
  } catch (err) {
    stageLabel.textContent = "Stopped";
    renderError(String(err));
  } finally {
    running = false;
    updateRunState();
  }
}

async function openReport() {
  if (!lastResult?.report_html) {
    showInlineError("Run a check first.");
    return;
  }
  try {
    await tauri().opener.openPath(lastResult.report_html);
  } catch (err) {
    showInlineError(`Open report failed: ${err}`);
  }
}

async function openFolder() {
  if (!lastResult?.report_dir) {
    showInlineError("Run a check first.");
    return;
  }
  try {
    await tauri().opener.revealItemInDir(lastResult.report_json);
  } catch (err) {
    showInlineError(`Open folder failed: ${err}`);
  }
}

async function publishVerification() {
  if (publishing || !lastResult?.report_json) return;
  publishing = true;
  publishBtn.disabled = true;
  publishStatus.textContent = "Publishing…";
  publishStatus.classList.remove("hidden");
  publishResult.classList.add("hidden");

  try {
    const result = await tauri().core.invoke("publish_verification", {
      reportJson: lastResult.report_json,
      artistName: artistInput.value.trim(),
      songTitle: songInput.value.trim(),
    });
    renderPublishResult(result);
  } catch (err) {
    publishStatus.textContent = String(err);
    publishStatus.classList.remove("hidden");
  } finally {
    publishing = false;
    updatePublishState();
  }
}

async function openPublishedPage() {
  if (!lastPublish?.url) return;
  try {
    await tauri().opener.openUrl(lastPublish.url);
  } catch (err) {
    showInlineError(`Open page failed: ${err}`);
  }
}

async function openPublishedQr() {
  if (!lastPublish?.qr_url) return;
  try {
    await tauri().opener.openUrl(lastPublish.qr_url);
  } catch (err) {
    showInlineError(`Open QR failed: ${err}`);
  }
}

async function copyPublishedUrl() {
  if (!lastPublish?.url) return;
  try {
    await navigator.clipboard.writeText(lastPublish.url);
    copyUrlBtn.textContent = "Copied";
    setTimeout(() => {
      copyUrlBtn.textContent = "Copy link";
    }, 1500);
  } catch (err) {
    showInlineError(`Copy failed: ${err}`);
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  try {
    tauri();
  } catch (err) {
    renderError(String(err));
    return;
  }

  document.querySelector("#pick-project").addEventListener("click", pickProject);
  document.querySelector("#pick-audio").addEventListener("click", pickAudio);
  runButton.addEventListener("click", runVerification);
  openReportBtn.addEventListener("click", openReport);
  openFolderBtn.addEventListener("click", openFolder);
  publishBtn.addEventListener("click", publishVerification);
  artistInput.addEventListener("input", updatePublishState);
  songInput.addEventListener("input", updatePublishState);
  openPageBtn.addEventListener("click", openPublishedPage);
  openQrBtn.addEventListener("click", openPublishedQr);
  copyUrlBtn.addEventListener("click", copyPublishedUrl);

  await tauri().event.listen("verify-progress", (event) => {
    handleProgress(event.payload);
  });

  updateRunState();
});
