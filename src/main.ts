import { invoke } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

type DisplayKind = "builtIn" | "physical" | "virtual" | "unknown";
type Display = { id: number; kind: DisplayKind; builtIn: boolean; main: boolean; active: boolean; online: boolean; width: number; height: number };
type DisplayStatus = { displays: Display[]; hasBuiltIn: boolean; externalCount: number; virtualCount: number; unknownCount: number; canSafelyDisconnect: boolean; platformSupported: boolean };
type EngineStatus = { available: boolean; symbol: string | null; testRunning: boolean; automationEnabled: boolean };

const headline = document.querySelector<HTMLElement>("#headline")!;
const summary = document.querySelector<HTMLElement>("#summary")!;
const list = document.querySelector<HTMLElement>("#display-list")!;
const dot = document.querySelector<HTMLElement>("#status-dot")!;
const refreshButton = document.querySelector<HTMLButtonElement>("#refresh")!;
const platform = document.querySelector<HTMLElement>("#platform")!;
const testButton = document.querySelector<HTMLButtonElement>("#test-disconnect")!;
const testCopy = document.querySelector<HTMLElement>("#test-copy")!;
const powerLabel = document.querySelector<HTMLElement>("#power-label")!;
const launchAtLogin = document.querySelector<HTMLInputElement>("#launch-at-login")!;
let currentStatus: DisplayStatus | null = null;
let automationEnabled = false;

function displayCard(display: Display): string {
  const kind = display.kind === "builtIn" ? "Built-in Retina display"
    : display.kind === "physical" ? "Physical external display"
    : display.kind === "virtual" ? "Virtual display"
    : "Unknown display";
  const badges = [
    display.main ? '<span class="badge">Main</span>' : "",
    !display.active ? '<span class="badge warn">Inactive</span>' : "",
    display.kind === "builtIn" ? '<span class="badge muted">Internal</span>'
      : display.kind === "physical" ? '<span class="badge safe">Physical</span>'
      : display.kind === "virtual" ? '<span class="badge muted">Virtual</span>'
      : '<span class="badge warn">Unknown</span>',
  ].join("");
  return `<article class="display-card">
    <div class="display-icon ${display.builtIn ? "laptop" : "monitor"}"></div>
    <div class="display-copy"><h4>${kind}</h4><p>${display.width} × ${display.height} · Display ${display.id}</p></div>
    <div class="badges">${badges}</div>
  </article>`;
}

async function refresh(): Promise<void> {
  refreshButton.disabled = true;
  refreshButton.textContent = "Checking…";
  try {
    const status = await invoke<DisplayStatus>("display_status");
    currentStatus = status;
    list.innerHTML = status.displays.map(displayCard).join("") || '<p class="empty">No online displays reported.</p>';
    platform.textContent = status.platformSupported ? "Apple silicon · supported" : "Unsupported platform";
    if (status.canSafelyDisconnect) {
      headline.textContent = "Ready for the safety engine";
      summary.textContent = `${status.externalCount} physical external display${status.externalCount === 1 ? " is" : "s are"} active. Virtual displays remain independent.`;
      dot.className = "status-dot ready";
      dot.setAttribute("aria-label", "Ready");
    } else {
      headline.textContent = "Keep the internal display connected";
      const ignored = status.virtualCount + status.unknownCount;
      summary.textContent = ignored > 0
        ? `${ignored} virtual or unverified display${ignored === 1 ? " is" : "s are"} being ignored for safety. Connect a confirmed physical monitor to continue.`
        : "Broken Screen will never disconnect the only usable display. Connect a physical monitor to continue.";
      dot.className = "status-dot caution";
      dot.setAttribute("aria-label", "External display required");
    }
  } catch (error) {
    headline.textContent = "Display check failed";
    summary.textContent = String(error);
    list.innerHTML = '<p class="empty">Core Graphics did not return a display list.</p>';
    dot.className = "status-dot error";
  } finally {
    refreshButton.disabled = false;
    refreshButton.textContent = "Refresh";
  }
}

async function refreshEngine(): Promise<void> {
  try {
    const engine = await invoke<EngineStatus>("engine_status");
    automationEnabled = engine.automationEnabled;
    testButton.disabled = !engine.available || (!automationEnabled && (currentStatus?.externalCount ?? 0) === 0);
    powerLabel.textContent = automationEnabled ? "ON" : "OFF";
    testButton.classList.toggle("enabled", automationEnabled);
    testButton.setAttribute("aria-pressed", String(automationEnabled));
    testCopy.textContent = automationEnabled
      ? "On · your broken screen is out of the desktop."
      : "Your Mac still works. Make macOS forget the broken screen.";
    if (!engine.available) testCopy.textContent = "The required private macOS display API is unavailable on this system.";
  } catch (error) {
    testButton.disabled = true;
    testCopy.textContent = String(error);
  }
}

async function toggleAutomation(): Promise<void> {
  testButton.disabled = true;
  try {
    await invoke("set_automation", { enabled: !automationEnabled });
  } catch (error) {
    testCopy.textContent = `Automation failed: ${String(error)}`;
  }
  await new Promise((resolve) => window.setTimeout(resolve, 500));
  await refresh();
  await refreshEngine();
}

async function loadLaunchAtLogin(): Promise<void> {
  try {
    launchAtLogin.checked = await isEnabled();
  } catch {
    launchAtLogin.disabled = true;
  }
}

launchAtLogin.addEventListener("change", async () => {
  launchAtLogin.disabled = true;
  try {
    if (launchAtLogin.checked) await enable();
    else await disable();
  } catch {
    launchAtLogin.checked = !launchAtLogin.checked;
  } finally {
    launchAtLogin.disabled = false;
  }
});

refreshButton.addEventListener("click", async () => { await refresh(); await refreshEngine(); });
testButton.onclick = toggleAutomation;
window.addEventListener("DOMContentLoaded", async () => { await refresh(); await refreshEngine(); await loadLaunchAtLogin(); });
window.addEventListener("focus", async () => { await refresh(); await refreshEngine(); });
