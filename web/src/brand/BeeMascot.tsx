export type BeeRole = "worker" | "queen" | "drone";
export type BeeExpression = "available" | "focused" | "thinking" | "blocked" | "complete" | "sleeping";

type Props = {
  role?: BeeRole;
  expression?: BeeExpression;
  className?: string;
  label?: string;
};

export default function BeeMascot({ role = "worker", expression = "available", className = "", label }: Props) {
  const female = role !== "drone";
  const sleeping = expression === "sleeping";
  const queen = role === "queen";
  const drone = role === "drone";
  return (
    <svg
      className={`bee-mascot bee-${role} bee-${expression} ${className}`.trim()}
      viewBox="0 0 120 120"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {sleeping && <path className="bee-leaf" d="M18 96C43 77 79 75 108 91 88 111 48 116 18 96Z" />}
      <g className="bee-wings">
        <ellipse cx={queen ? 34 : 38} cy="47" rx={queen ? 23 : 19} ry={queen ? 12 : 11} transform="rotate(-38 38 47)" />
        <ellipse cx={queen ? 85 : 80} cy="45" rx={queen ? 23 : 18} ry={queen ? 12 : 10} transform="rotate(35 80 45)" />
        <path d="M25 46 45 48M72 47l20-4" />
      </g>
      <path className="bee-abdomen" d={drone ? "M35 68C38 52 79 50 91 72 95 84 87 104 66 109 48 108 31 89 35 68Z" : queen ? "M40 66C45 51 76 50 83 67 89 86 76 110 61 115 45 106 34 84 40 66Z" : "M39 66C43 52 76 51 83 68 88 84 77 106 61 111 45 104 34 83 39 66Z"} />
      <path className="bee-stripe" d="M38 75c14 8 33 9 47 1M40 91c12 7 27 8 40 2" />
      <path className="bee-leg" d={sleeping ? "M41 91c-8 5-12 8-17 6M77 94c7 3 12 4 18 2" : "M46 96c-4 9-7 13-12 16M74 97c3 8 7 12 12 15"} />
      <circle className="bee-head" cx="61" cy="43" r={drone ? 27 : 26} />
      <path className="bee-collar" d="M38 62c5-8 11-9 16-4 5-6 11-6 16 0 6-5 12-2 15 5-8 8-37 9-47-1Z" />
      <g className="bee-antennae">
        <path d={drone ? "M48 21 42 5M72 20 79 5" : "M49 20C47 12 42 8 38 5M72 20c3-8 8-12 13-14"} />
        <circle cx={drone ? 42 : 38} cy="5" r="3" />
        <circle cx={drone ? 79 : 85} cy={drone ? 5 : 6} r="3" />
      </g>
      {queen && <g className="bee-diadem"><path d="m47 19 2-11 8 8 4-13 5 13 9-8-1 12" /><circle cx="61" cy="7" r="2.5" /></g>}
      <BeeFace expression={expression} female={female} />
      {!sleeping && <path className="bee-arm" d={expression === "thinking" ? "M77 69c10-4 11-11 7-17" : expression === "blocked" ? "M78 70c10-4 14-9 17-16" : "M43 70c-8 4-12 8-14 14"} />}
      {expression === "complete" && <g className="bee-complete-mark"><circle cx="94" cy="82" r="16" /><path d="m86 82 6 6 11-14" /></g>}
      {expression === "blocked" && <g className="bee-blocked-mark"><path d="M91 79h19M100.5 69v20" /></g>}
      {sleeping && <g className="bee-sleep-mark"><path d="m88 35 10-8H88l10-8" /><path d="m99 48 7-6h-7l7-6" /></g>}
    </svg>
  );
}

function BeeFace({ expression, female }: { expression: BeeExpression; female: boolean }) {
  if (expression === "sleeping" || expression === "complete") {
    return <g className="bee-face"><path className="bee-eye-line" d="M48 42q5 5 10 0M66 42q5 5 10 0" />{female && <path className="bee-lashes" d="m48 41-3-2m31 2 3-2" />}<path className="bee-mouth" d="M57 52q5 5 10 0" /></g>;
  }
  const focused = expression === "focused";
  const blocked = expression === "blocked";
  return <g className="bee-face">
    <ellipse className="bee-eye" cx="52" cy="42" rx={focused ? 3.4 : 4} ry={focused ? 4.5 : 5} />
    <ellipse className="bee-eye" cx="71" cy="42" rx={focused ? 3.4 : 4} ry={focused ? 4.5 : 5} />
    <circle className="bee-eye-glint" cx="53" cy="40" r="1" /><circle className="bee-eye-glint" cx="72" cy="40" r="1" />
    {female && <path className="bee-lashes" d="m48 37-3-2m3 5-4-1m31-2 3-2m-3 5 4-1" />}
    {focused && <path className="bee-brow" d="m47 34 9 1m11 0 9-1" />}
    {blocked && <path className="bee-brow" d="m47 34 8-2m12 0 8 3" />}
    <path className="bee-mouth" d={focused ? "M58 53h7" : blocked ? "M57 55q5-5 10 0" : expression === "thinking" ? "M58 53q5 3 9-1" : "M57 52q5 6 11 0"} />
  </g>;
}
