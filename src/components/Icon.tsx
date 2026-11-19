const paths = {
  chat: "M4 5h16v11H9l-5 4V5Z",
  group: "M8 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM16 11a3 3 0 1 0 0-6M2 20v-2a5 5 0 0 1 10 0v2M15 14a5 5 0 0 1 7 4v2",
  search: "M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM16 16l5 5",
  settings: "m10 3-.6 3-2.4 1-2.5-1.1-2 3.5L5 11v2l-2.5 1.6 2 3.5L7 17l2.4 1 .6 3h4l.6-3 2.4-1 2.5 1.1 2-3.5L19 13v-2l2.5-1.6-2-3.5L17 7l-2.4-1L14 3h-4ZM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z",
  plus: "M12 5v14M5 12h14",
  folder: "M3 6h7l2 2h9v12H3V6Z",
  down: "m7 10 5 5 5-5",
  right: "m9 6 6 6-6 6",
  send: "m12 19 0-14m-6 6 6-6 6 6",
  attach: "m9 16 7-7a2 2 0 0 0-3-3l-8 8a4 4 0 0 0 6 6l9-9a6 6 0 0 0-9-9L3 10",
  at: "M16 8v7c0 3 5 2 5-3a9 9 0 1 0-5 8M16 12a4 4 0 1 0-8 0 4 4 0 0 0 8 0Z",
  close: "m6 6 12 12M6 18 18 6",
  copy: "M8 8h12v13H8V8ZM16 8V3H3v13h5",
  reply: "m9 5-6 6 6 6M3 11h10a7 7 0 0 1 7 7v2",
  check: "m5 12 4 4L19 6",
  more: "M5 12h.01M12 12h.01M19 12h.01",
  panel: "M3 4h18v16H3V4ZM15 4v16",
  stop: "M6 6h12v12H6V6Z",
  file: "M5 3h9l5 5v13H5V3ZM14 3v6h5M9 13h6M9 17h6",
  terminal: "m5 6 5 5-5 5M13 17h6",
  shield: "m12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Zm-4 9 3 3 5-6",
  back: "m14 5-7 7 7 7",
  spark: "m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3Z",
};
export type IconName = keyof typeof paths;
export default function Icon({ name, size = 20 }: { name: IconName; size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}
