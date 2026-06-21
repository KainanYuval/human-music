function tauri() {
  if (!window.__TAURI__) {
    throw new Error("Tauri API not available. Run with: cd app && npm run dev");
  }
  return window.__TAURI__;
}

const projectInput = document.querySelector("#project-path");
const audioInput = document.querySelector("#audio-path");
const runButton = document.querySelector("#run-verify");
const progressPanel = document.querySelector("#progress-panel");
const progressBar = document.querySelector("#progress-bar");
const stageLabel = document.querySelector("#stage-label");
const progressPercent = document.querySelector("#progress-percent");
const subProgress = document.querySelector("#sub-progress");
const stepBar = document.querySelector("#step-bar");
const stepLabel = document.querySelector("#step-label");
const stepPercent = document.querySelector("#step-percent");
const resultPanel = document.querySelector("#result-panel");
const verdictBadge = document.querySelector("#verdict-badge");
const claimText = document.querySelector("#claim-text");
const coverageStat = document.querySelector("#coverage-stat");
const strongStat = document.querySelector("#strong-stat");
const possibleStat = document.querySelector("#possible-stat");
const bestMatch = document.querySelector("#best-match");
const errorText = document.querySelector("#error-text");
const openReportBtn = document.querySelector("#open-report");
const openFolderBtn = document.querySelector("#open-folder");

let lastResult = null;
let running = false;
let estimatedSeconds = null;

function updateRunState() {
  runButton.disabled =
    running || !projectInput.value.trim() || !audioInput.value.trim();
}

function formatProgressLabel(percent) {
  const pct = Math.round(Math.max(0, Math.min(1, percent)) * 100);
  if (estimatedSeconds && percent > 0 && percent < 1) {
    const remaining = Math.max(0, estimatedSeconds * (1 - percent));
    return `${pct}% · ~${Math.ceil(remaining)}s left`;
  }
  return `${pct}%`;
}

function setBar(bar, pctEl, fraction) {
  const pct = Math.round(Math.max(0, Math.min(1, fraction)) * 100);
  bar.value = pct;
  pctEl.textContent = `${pct}%`;
}

function resetProgress() {
  estimatedSeconds = null;
  setBar(progressBar, progressPercent, 0);
  stageLabel.textContent = "Starting…";
  subProgress.classList.add("hidden");
  setBar(stepBar, stepPercent, 0);
  stepLabel.textContent = "—";
}

function handleProgress(evt) {
  if (evt.estimated_seconds != null) {
    estimatedSeconds = evt.estimated_seconds;
  }

  const overall = evt.percent ?? 0;
  setBar(progressBar, progressPercent, overall);
  progressPercent.textContent = formatProgressLabel(overall);
  stageLabel.textContent = evt.stage_label || evt.message || "Working…";

  const hasStep =
    evt.step_percent != null && evt.step && evt.step !== "done" && evt.step !== "complete";
  if (hasStep) {
    subProgress.classList.remove("hidden");
    setBar(stepBar, stepPercent, evt.step_percent);
    const itemPrefix =
      evt.item_name && evt.item_total != null && evt.item_index != null
        ? `[${evt.item_index + 1}/${evt.item_total}] ${evt.item_name} — `
        : "";
    stepLabel.textContent = `${itemPrefix}${evt.message}${evt.detail ? ` — ${evt.detail}` : ""}`;
  } else if (evt.stage === "done") {
    subProgress.classList.add("hidden");
  }
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

function renderResult(result) {
  lastResult = result;
  errorText.classList.add("hidden");
  errorText.textContent = "";

  verdictBadge.textContent = result.verdict;
  verdictBadge.className = `badge ${result.verdict.toLowerCase()}`;

  claimText.textContent = result.claim;
  coverageStat.textContent = `${(result.coverage_ratio * 100).toFixed(1)}%`;
  strongStat.textContent = String(result.strong_match_count);
  possibleStat.textContent = String(result.possible_match_count);

  if (result.best_match) {
    const b = result.best_match;
    bestMatch.textContent = `Best asset: ${b.asset} @ ${b.offset_seconds.toFixed(
      2,
    )}s (score ${b.match_score.toFixed(3)}, ${b.status})`;
  } else {
    bestMatch.textContent = "No asset match found.";
  }

  showResultPanel();
}

function renderError(message) {
  lastResult = null;
  verdictBadge.textContent = "ERROR";
  verdictBadge.className = "badge error";
  claimText.textContent = "";
  coverageStat.textContent = "—";
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
        title: "Select GarageBand project (.band)",
        filters: [{ name: "GarageBand Project", extensions: ["band"] }],
      }),
    );
    if (path) {
      if (!path.toLowerCase().endsWith(".band")) {
        renderError("Please select a .band GarageBand project.");
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
        title: "Select released audio (WAV or MP3)",
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
      message: "Verification complete",
      step: "complete",
    });
    renderResult(result);
  } catch (err) {
    stageLabel.textContent = "Failed";
    renderError(String(err));
  } finally {
    running = false;
    updateRunState();
  }
}

async function openReport() {
  if (!lastResult?.report_html) {
    showInlineError("No report yet. Run verification first.");
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
    showInlineError("No report folder yet. Run verification first.");
    return;
  }
  try {
    await tauri().opener.revealItemInDir(lastResult.report_json);
  } catch (err) {
    showInlineError(`Open folder failed: ${err}`);
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

  await tauri().event.listen("verify-progress", (event) => {
    handleProgress(event.payload);
  });

  updateRunState();
});
