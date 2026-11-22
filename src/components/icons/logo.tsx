export const Logo = (props: React.SVGProps<SVGSVGElement>) => (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width={120}
    height={120}
    role="graphics-symbol img"
    fill="none"
    stroke="currentColor"
    strokeLinecap="round"
    viewBox="0 0 120 120"
    {...props}
  >
    <title>NullFrame</title>
    <rect x="16" y="16" width="88" height="88" rx="22" strokeWidth="10" />
    <circle cx="60" cy="60" r="23" strokeWidth="9" />
    <line x1="35" y1="85" x2="85" y2="35" strokeWidth="9" />
  </svg>
);
