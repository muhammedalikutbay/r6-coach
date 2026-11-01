// Bumped after a successful `refresh`; views read `refreshTick.value` inside their load effect to re-query.
class RefreshTick {
  value = $state(0);
  bump() {
    this.value += 1;
  }
}

export const refreshTick = new RefreshTick();
