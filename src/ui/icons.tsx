import type { ReactNode } from 'react';

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  );
}

export const PlayIcon = () => (
  <Icon>
    <path d="M9 6.5v11l9-5.5z" />
  </Icon>
);
export const PauseIcon = () => (
  <Icon>
    <path d="M9.5 6.5v11M14.5 6.5v11" />
  </Icon>
);
export const StepIcon = () => (
  <Icon>
    <path d="M7 6.5v11l8-5.5zM18 6.5v11" />
  </Icon>
);
export const BrushIcon = () => (
  <Icon>
    <path d="M19 4.5 10.5 13M8.5 13.5c-2 0-3 1.5-3 3 0 1.3-.6 2.2-1.5 3 3 .4 7.5-.5 7.5-3.5a2.8 2.8 0 0 0-3-2.5z" />
  </Icon>
);
export const EraserIcon = () => (
  <Icon>
    <path d="m13.5 5.5 5.5 5.5-8 8H7l-3-3zM9 10l5.5 5.5M7 19h13" />
  </Icon>
);
export const HandIcon = () => (
  <Icon>
    <path d="M12 3.5v17M3.5 12h17M12 3.5 9.5 6M12 3.5 14.5 6M12 20.5 9.5 18M12 20.5l2.5-2.5M3.5 12 6 9.5M3.5 12 6 14.5M20.5 12 18 9.5M20.5 12 18 14.5" />
  </Icon>
);
export const StampIcon = () => (
  <Icon>
    <path d="M5 5h4v4H5zM15 5h4v4h-4zM10 10h4v4h-4zM5 15h4v4H5zM15 15h4v4h-4z" />
  </Icon>
);
export const ChevronIcon = ({ open }: { open: boolean }) => (
  <Icon>
    <path d={open ? 'm8 14 4-4 4 4' : 'm8 10 4 4 4-4'} />
  </Icon>
);
export const SeedIcon = () => (
  <Icon>
    <path d="M6 7h.01M12 5h.01M18 8h.01M8 12h.01M15 12h.01M5 17h.01M11 18h.01M18 16h.01" strokeWidth="2.2" />
  </Icon>
);
export const SunIcon = () => (
  <Icon>
    <circle cx="12" cy="12" r="3.5" />
    <path d="M12 4v2M12 18v2M4 12h2M18 12h2M6.3 6.3l1.4 1.4M16.3 16.3l1.4 1.4M6.3 17.7l1.4-1.4M16.3 7.7l1.4-1.4" />
  </Icon>
);
export const MoonIcon = () => (
  <Icon>
    <path d="M19 14.5A7.5 7.5 0 0 1 9.5 5a7.5 7.5 0 1 0 9.5 9.5z" />
  </Icon>
);
export const ClearIcon = () => (
  <Icon>
    <path d="m7 7 10 10M17 7 7 17" />
  </Icon>
);
