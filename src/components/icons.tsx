// Lucide icons (https://lucide.dev, ISC licence) as react-native-svg components.
// React Native can't render <svg> tags directly; these are the same paths.
// To add one: copy its <path>/<circle> elements from lucide.dev into a new
// component below (attributes are camelCase here: strokeWidth, not stroke-width).

import Svg, { Circle, Path } from "react-native-svg";
import type { ReactNode } from "react";

export type IconProps = { size?: number; color?: string; strokeWidth?: number };

function Icon({ size = 24, color = "#000", strokeWidth = 2, children }: IconProps & { children: ReactNode }) {
    return (
        <Svg
            width={size}
            height={size}
            viewBox="0 0 24 24"
            fill="none"
            stroke={color}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            strokeLinejoin="round"
        >
            {children}
        </Svg>
    );
}

export const ChevronLeftIcon = (p: IconProps) => (
    <Icon {...p}><Path d="m15 18-6-6 6-6" /></Icon>
);

export const ChevronRightIcon = (p: IconProps) => (
    <Icon {...p}><Path d="m9 18 6-6-6-6" /></Icon>
);

export const SearchIcon = (p: IconProps) => (
    <Icon {...p}><Path d="m21 21-4.34-4.34" /><Circle cx="11" cy="11" r="8" /></Icon>
);

export const MenuIcon = (p: IconProps) => (
    <Icon {...p}><Path d="M4 5h16" /><Path d="M4 12h16" /><Path d="M4 19h16" /></Icon>
);

export const XIcon = (p: IconProps) => (
    <Icon {...p}><Path d="M18 6 6 18" /><Path d="m6 6 12 12" /></Icon>
);

export const HouseIcon = (p: IconProps) => (
    <Icon {...p}>
        <Path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8" />
        <Path d="M3 10a2 2 0 0 1 .709-1.528l7-6a2 2 0 0 1 2.582 0l7 6A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </Icon>
);

export const LogOutIcon = (p: IconProps) => (
    <Icon {...p}>
        <Path d="m16 17 5-5-5-5" />
        <Path d="M21 12H9" />
        <Path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
    </Icon>
);
