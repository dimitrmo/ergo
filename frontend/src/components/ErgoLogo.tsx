import './ErgoLogo.css';

/**
 * The ergo mark.
 *
 * "ergo" means "therefore", and the therefore sign (∴) is three dots. Here the
 * dots are a tiny workflow: a trigger (solid, lower left) flows up into a step
 * (solid, top) and on to an output (open ring, lower right). One path, one
 * direction.
 *
 * Treatment matches the phirepass mark: a neon sweep on a near-black body, a
 * bloom and a lit edge. The sweep is rotated into ergo's amber so the mark
 * sits on brand with the UI accent (#ffb000).
 *
 * Animated, the mark shows a run: the trigger fires, a packet travels the path,
 * the step lights as the packet passes, and the output ring pulses as it lands.
 * Every animation stops under prefers-reduced-motion, and the mark stays fully
 * legible static.
 *
 * Gradient ids are static. Several instances on one page define the same
 * gradients, so the duplicate ids resolve identically.
 */

// Trigger -> step -> output. `pathLength` normalises it to 100 so the packet
// timing in ErgoLogo.css doesn't depend on the curve's real length.
const FLOW = 'M13 33.5 C 13.6 25.2, 17.4 17.6, 24 14.5 C 30.2 17.2, 34 23.4, 34.8 28.9';

export function ErgoLogo({
    className = '',
    title,
    animated = true,
}: {
    className?: string;
    /** Accessible name. Omit when a text wordmark sits beside the mark. */
    title?: string;
    /** Set false for static contexts (e.g. dense lists) where motion is noise. */
    animated?: boolean;
}) {
    return (
        <svg
            viewBox="0 0 48 48"
            className={`er-logo${animated ? ' er-animated' : ''} ${className}`}
            role={title ? 'img' : 'presentation'}
            aria-hidden={title ? undefined : true}
            xmlns="http://www.w3.org/2000/svg"
        >
            {title ? <title>{title}</title> : null}
            <defs>
                {/* userSpaceOnUse, so one sweep runs across the whole mark:
                    the trigger sits in the orange end, the output in the
                    yellow end. */}
                <linearGradient
                    id="er-mark"
                    gradientUnits="userSpaceOnUse"
                    x1="8"
                    y1="41"
                    x2="40"
                    y2="9"
                >
                    <stop offset="0%" stopColor="hsl(18 96% 54%)" />
                    <stop offset="35%" stopColor="hsl(30 98% 55%)" />
                    <stop offset="70%" stopColor="hsl(41 100% 55%)" />
                    <stop offset="100%" stopColor="hsl(52 100% 64%)" />
                </linearGradient>
                <linearGradient id="er-body" x1="0%" y1="0%" x2="100%" y2="100%">
                    <stop offset="0%" stopColor="hsl(28 30% 13%)" />
                    <stop offset="50%" stopColor="hsl(34 32% 9%)" />
                    <stop offset="100%" stopColor="hsl(42 36% 7%)" />
                </linearGradient>
                {/* Bloom behind the step node, where the flow turns. */}
                <radialGradient id="er-bloom" cx="50%" cy="34%" r="60%">
                    <stop offset="0%" stopColor="hsl(41 100% 55%)" stopOpacity="0.34" />
                    <stop offset="100%" stopColor="hsl(41 100% 55%)" stopOpacity="0" />
                </radialGradient>
                <linearGradient id="er-edge" x1="0%" y1="100%" x2="100%" y2="0%">
                    <stop offset="0%" stopColor="hsl(18 96% 54%)" stopOpacity="0.8" />
                    <stop offset="100%" stopColor="hsl(52 100% 64%)" stopOpacity="0.55" />
                </linearGradient>
            </defs>

            {/* Body: near-black, faintly warm, with a lit edge. */}
            <rect x="1" y="1" width="46" height="46" rx="12" fill="url(#er-body)" />
            <rect x="1" y="1" width="46" height="46" rx="12" fill="url(#er-bloom)" />
            <rect
                x="1"
                y="1"
                width="46"
                height="46"
                rx="12"
                fill="none"
                stroke="url(#er-edge)"
                strokeWidth="1.8"
            />

            {/* The rail: the workflow's one path. Dimmed only when animated,
                so the travelling packet reads against it. */}
            <path
                className="er-rail"
                d={FLOW}
                fill="none"
                stroke="url(#er-mark)"
                strokeWidth="2.6"
                strokeLinecap="round"
                strokeLinejoin="round"
            />

            {/* The packet: a short dash that runs trigger -> step -> output. */}
            {animated ? (
                <path
                    className="er-packet"
                    d={FLOW}
                    pathLength={100}
                    fill="none"
                    stroke="url(#er-mark)"
                    strokeWidth="3.2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                />
            ) : null}

            {/* Trigger: solid, fires at the start of each run. */}
            <circle className="er-trigger" cx="13" cy="33.5" r="3.8" fill="url(#er-mark)" />

            {/* Step: solid, lights as the packet passes through. */}
            <circle className="er-step" cx="24" cy="14.5" r="3.8" fill="url(#er-mark)" />

            {/* Output: open, pulses as the packet lands. */}
            <circle
                className="er-output"
                cx="35"
                cy="33.2"
                r="4.1"
                fill="none"
                stroke="url(#er-mark)"
                strokeWidth="2.4"
            />
        </svg>
    );
}
