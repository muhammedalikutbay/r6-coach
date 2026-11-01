// First-run welcome screen state. "Seen" is kept in localStorage, which lives in
// the WebView2 app data folder, so it survives restarts.
const KEY = 'r6coach.welcomeSeen';

function wasSeen(): boolean {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch (e) {
    // Storage can be blocked. Fall back to showing the screen once per launch.
    console.warn('welcome: localStorage unavailable', e);
    return false;
  }
}

function markSeen(): void {
  try {
    localStorage.setItem(KEY, '1');
  } catch (e) {
    console.warn('welcome: could not persist "seen" flag', e);
  }
}

export const welcome = $state({ open: !wasSeen() });

export function dismissWelcome(): void {
  markSeen();
  welcome.open = false;
}

export function showWelcomeAgain(): void {
  welcome.open = true;
}
