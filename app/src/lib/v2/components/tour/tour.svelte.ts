// First-run guided tour: state and steps. The tour spotlights the sidebar links (`data-tour="<id>"`
// set in the root layout), one by one. "Seen" is kept in localStorage like the welcome screen.
const KEY = 'r6coach.tourSeen';

export interface TourStep {
  /** `data-tour` id of the sidebar link to spotlight; null = a centered card. */
  target: string | null;
  title: string;
  text: string;
}

export const TOUR_STEPS: TourStep[] = [
  {
    target: 'home',
    title: 'Home',
    text: 'Your coach greets you here: your last match, what to work on next, and how your form compares with your own history.'
  },
  {
    target: 'overview',
    title: 'Overview',
    text: 'All your numbers in one place: K/D, win rate, headshots, maps and operators. The filter bar at the top narrows them down.'
  },
  {
    target: 'matches',
    title: 'Matches',
    text: 'Every match from your replays, grouped by day. Open one to see the scoreboard, every round and your kill clips.'
  },
  {
    target: 'operators',
    title: 'Operators',
    text: 'How you play each operator: rounds, win rate, K/D, best maps and weapons.'
  },
  {
    target: 'weapons',
    title: 'Weapons',
    text: 'Which weapons get you kills, your headshot rate with each, and the operators that carry them.'
  },
  {
    target: 'coach',
    title: 'Coach',
    text: 'Compares your last matches with your own history and tells you what to praise and what to fix, most important first.'
  },
  {
    target: 'profile',
    title: 'Profile',
    text: 'Your rank and season history from Arenyze. This is the only page that needs the optional Arenyze key.'
  },
  {
    target: 'settings',
    title: 'Settings',
    text: 'Pick your replay folder, add the Arenyze key, turn on kill clips, and replay this tour whenever you like.'
  },
  {
    target: null,
    title: 'R6 Coach keeps running in the tray',
    text: 'Closing the window does not quit the app: it keeps importing your matches from the system tray. To quit it fully, right-click the tray icon and choose Quit.'
  }
];


function markSeen(): void {
  try {
    localStorage.setItem(KEY, '1');
  } catch (e) {
    console.warn('tour: could not persist "seen" flag', e);
  }
}

export const tour = $state({ open: false, step: 0 });

/** Opens the tour at the first step (after the welcome screen, or from Settings). */
export function startTour(): void {
  tour.step = 0;
  tour.open = true;
}

/** Closes the tour (finished or skipped) and remembers it was seen. */
export function closeTour(): void {
  markSeen();
  tour.open = false;
}

