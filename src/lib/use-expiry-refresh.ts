import { useEffect, useState } from "react";

export function useExpiryRefresh() {
  const [, update] = useState(0);
  useEffect(() => {
    const refresh = () => update((value) => value + 1);
    const timer = window.setInterval(refresh, 60_000);
    window.addEventListener("focus", refresh);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
    };
  }, []);
}
