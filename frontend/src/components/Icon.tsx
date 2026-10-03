// Small stroke icons, drawn to match the UI (1.75 px strokes, round caps).

const PATHS: Record<string, string> = {
  bolt: 'M13 3 5 13h6l-1 8 8-10h-6l1-8Z',
  clock: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm0-13v4.5l3 2',
  hand: 'M8 13V5.5a1.5 1.5 0 0 1 3 0V12m0-1V4.5a1.5 1.5 0 0 1 3 0V12m0-1.5V6a1.5 1.5 0 0 1 3 0v8a7 7 0 0 1-7 7h-.5a6 6 0 0 1-4.9-2.6L4.2 15a1.5 1.5 0 0 1 2.4-1.8L8 15',
  send: 'M21 3 10 14M21 3l-7 18-4-7-7-4 18-7Z',
  globe: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm-9-9h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3Z',
  download: 'M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 19h14',
  upload: 'M12 15V4m0 0L7.5 8.5M12 4l4.5 4.5M5 19h14',
  braces: 'M8 4H7a2 2 0 0 0-2 2v4l-2 2 2 2v4a2 2 0 0 0 2 2h1m8-16h1a2 2 0 0 1 2 2v4l2 2-2 2v4a2 2 0 0 1-2 2h-1',
  funnel: 'M4 5h16l-6 7.5V19l-4 1.5v-8L4 5Z',
  shuffle: 'M4 7h4.5c2 0 3 1 4 2.5l1.5 2.5c1 1.5 2 2.5 4 2.5H20m0 0-3-3m3 3-3 3M4 17h4.5c1.2 0 2-.4 2.7-1M20 7h-2c-1.2 0-2 .4-2.7 1M20 7l-3-3m3 3-3 3',
  compose: 'M4 20h4L19 9l-4-4L4 16v4Zm9-13 4 4',
  plus: 'M12 5v14M5 12h14',
  play: 'M7 4.5v15l12-7.5-12-7.5Z',
  check: 'm5 12.5 4.5 4.5L19 7.5',
  x: 'M6 6l12 12M18 6 6 18',
  back: 'M15 5l-7 7 7 7',
  history: 'M3 12a9 9 0 1 0 3-6.7L3 8m0-5v5h5m4-1v5l3.5 2',
  copy: 'M9 9V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-4M5 9h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2Z',
  branch: 'M6 3v12m0 0a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm12-6a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm0 0c0 4-4 4.5-12 6',
  home: 'M4 10.5 12 4l8 6.5V20a1 1 0 0 1-1 1h-4.5v-6h-5v6H5a1 1 0 0 1-1-1v-9.5Z',
  inbox: 'M4 13.5 6.5 5h11l2.5 8.5M4 13.5V19a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-5.5M4 13.5h4.5l1 2.5h5l1-2.5H20',
  trash: 'M4 7h16M10 11v6m4-6v6M6 7l1 13h10l1-13M9 7V4h6v3',
  dots: 'M5 12h.01M12 12h.01M19 12h.01',
  timer: 'M12 21a8 8 0 1 0 0-16 8 8 0 0 0 0 16Zm0-12v4l2.5 2.5M10 2.5h4',
  hourglass: 'M7 3h10M7 21h10M8 3v3a4 4 0 0 0 1.6 3.2L12 11l2.4-1.8A4 4 0 0 0 16 6V3M8 21v-3a4 4 0 0 1 1.6-3.2L12 13l2.4 1.8A4 4 0 0 1 16 18v3',
  browser: 'M4 5h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Zm-1 4h18M6 7h.01M8.5 7h.01',
  bell: 'M6 16v-5a6 6 0 1 1 12 0v5l1.5 2h-15L6 16Zm4 4a2 2 0 0 0 4 0',
  sparkle: 'M12 3v4m0 10v4M3 12h4m10 0h4M6.3 6.3l2.8 2.8m5.8 5.8 2.8 2.8m0-11.4-2.8 2.8m-5.8 5.8-2.8 2.8',
}

export type IconName = keyof typeof PATHS

export function Icon({ name, size = 18, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  )
}
