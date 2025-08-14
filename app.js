// --- DOM Elements ---
const setupScreen = document.getElementById("setup-screen");
const gameScreen = document.getElementById("game-screen");
const startButton = document.getElementById("start-game");
const hoursInput = document.getElementById("hours");
const minutesInput = document.getElementById("minutes");
const secondsInput = document.getElementById("seconds");
const backButton = document.getElementById("back-button");
const incrementMinutesInput = document.getElementById("increment-minutes");
const incrementSecondsInput = document.getElementById("increment-seconds");

const playerDivs = [
  document.getElementById("player-0"),
  document.getElementById("player-1"),
];
const playerSpans = [
  document.querySelector("#player-0 span"),
  document.querySelector("#player-1 span"),
];
const pauseResumeButton = document.getElementById("pause-resume");
const buttons = document.getElementById("buttons");

// --- State ---
let currentPlayer = 0;
let timers = [null, null];
let remainingTimes = [300000, 300000]; // milliseconds
let isRunning = false;
let wakeLock = null;
let incrementMs = 0;

// --- Functions ---
function formatTime(ms) {
  const m = Math.floor(ms / 60000)
    .toString()
    .padStart(2, "0");
  const seconds = ((ms % 60000) / 1000).toFixed(1).padStart(4, "0"); // e.g. "59.8"
  const [s, dec] = seconds.split(".");
  return `${m}:${s}.${dec}`;
}

function updateActivePlayerStyle() {
  for (let i = 0; i < 2; i++) {
    playerDivs[i].classList.remove("bg-gray-700", "bg-gray-800");
    playerDivs[i].classList.add(
      i === currentPlayer ? "bg-gray-700" : "bg-gray-800"
    );
  }
}

function updateDisplay() {
  for (let i = 0; i < 2; i++) {
    playerSpans[i].textContent = formatTime(remainingTimes[i]);
  }
}

function startTurn(player) {
  clearInterval(timers[0]);
  clearInterval(timers[1]);

  if (!isRunning) return;

  const start = performance.now();
  const initialRemaining = remainingTimes[player];

  timers[player] = setInterval(() => {
    const elapsed = performance.now() - start;
    const newRemaining = initialRemaining - elapsed;
    remainingTimes[player] = newRemaining;

    if (remainingTimes[player] <= 0) {
      remainingTimes[player] = 0;
      clearInterval(timers[player]);
      isRunning = false;

      // Add red background to the player who ran out of time
      playerDivs[player].classList.remove("bg-gray-700", "bg-gray-800");
      playerDivs[player].classList.add("bg-red-700");

      updateDisplay();
      return;
    }

    updateDisplay();
  }, 100); // update every 50ms
}

function switchPlayer() {
  if (!isRunning) return;

  // Add increment to the current player before switching
  remainingTimes[currentPlayer] += incrementMs;

  currentPlayer = currentPlayer === 0 ? 1 : 0;
  updateActivePlayerStyle();
  updateDisplay(); // Update immediately after increment

  startTurn(currentPlayer);
}

function startGame() {
  const h = parseInt(hoursInput.value, 10) || 0;
  const m = parseInt(minutesInput.value, 10) || 0;
  const s = parseInt(secondsInput.value, 10) || 0;
  const incMin = parseInt(incrementMinutesInput.value, 10) || 0;
  const incSec = parseInt(incrementSecondsInput.value, 10) || 0;
  incrementMs = (incMin * 60 + incSec) * 1000;

  const ms = (h * 3600 + m * 60 + s) * 1000;

  remainingTimes = [ms, ms];
  playerDivs.forEach((div) => {
    div.classList.remove("bg-red-700");
  });

  updateDisplay();
  setupScreen.classList.add("hidden");
  gameScreen.classList.remove("hidden");
  buttons.classList.remove("hidden");

  isRunning = false; // Don't start yet
  currentPlayer = null; // No active player yet
  updateActivePlayerStyle(); // Clear active styles
  enableWakeLock();
}

function pauseResume() {
  if (currentPlayer === null) return;
  if (isRunning) {
    clearInterval(timers[0]);
    clearInterval(timers[1]);
    isRunning = false;
    pauseResumeButton.textContent = "Resume";
  } else {
    isRunning = true;
    startTurn(currentPlayer);
    pauseResumeButton.textContent = "Pause";
  }
}

// --- Wake Lock ---
async function enableWakeLock() {
  try {
    wakeLock = await navigator.wakeLock.request("screen");
  } catch (err) {
    console.warn("Wake lock failed", err);
  }
}

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && isRunning) {
    enableWakeLock();
  }
});

// --- Event Listeners ---
startButton.addEventListener("click", startGame);
playerDivs[0].addEventListener("click", () => {
  if (!isRunning && currentPlayer === null) {
    currentPlayer = 1; // Start with Player 1's timer
    isRunning = true;
    startTurn(currentPlayer);
    updateActivePlayerStyle();
  } else if (currentPlayer === 0 && isRunning) {
    switchPlayer();
  }
});

playerDivs[1].addEventListener("click", () => {
  if (!isRunning && currentPlayer === null) {
    currentPlayer = 0; // Start with Player 0's timer
    isRunning = true;
    startTurn(currentPlayer);
    updateActivePlayerStyle();
  } else if (currentPlayer === 1 && isRunning) {
    switchPlayer();
  }
});

pauseResumeButton.addEventListener("click", pauseResume);

backButton.addEventListener("click", () => {
  // Stop timers
  clearInterval(timers[0]);
  clearInterval(timers[1]);
  isRunning = false;

  // Reset UI: show setup, hide game screen
  setupScreen.classList.remove("hidden");
  gameScreen.classList.add("hidden");
  buttons.classList.add("hidden");

  // Reset active player & styles
  currentPlayer = null;
  updateActivePlayerStyle();

  // Reset times (optional)
  remainingTimes = [0, 0];
  updateDisplay();

  // Remove any red background if present
  playerDivs.forEach((div) => {
    div.classList.remove("bg-red-700");
  });
});

// --- PWA Support ---
if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("sw.js");
}
