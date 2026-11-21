// Lucide's line icons (ISC, `icons/LICENSE`), unmodified, tinted with a theme token.

import { Image, type ImageStyle, type StyleProp } from "nsh";
import { token } from "./theme";
import arrowLeft from "./icons/arrow-left.svg";
import arrowRight from "./icons/arrow-right.svg";
import bold from "./icons/bold.svg";
import bot from "./icons/bot.svg";
import check from "./icons/check.svg";
import chevronDown from "./icons/chevron-down.svg";
import chevronLeft from "./icons/chevron-left.svg";
import chevronRight from "./icons/chevron-right.svg";
import chevronsUpDown from "./icons/chevrons-up-down.svg";
import circleAlert from "./icons/circle-alert.svg";
import code from "./icons/code.svg";
import copy from "./icons/copy.svg";
import cornerDownLeft from "./icons/corner-down-left.svg";
import ellipsis from "./icons/ellipsis.svg";
import fileDown from "./icons/file-down.svg";
import filePenLine from "./icons/file-pen-line.svg";
import fileText from "./icons/file-text.svg";
import filterX from "./icons/filter-x.svg";
import folder from "./icons/folder.svg";
import folderOpen from "./icons/folder-open.svg";
import folderPlus from "./icons/folder-plus.svg";
import gripVertical from "./icons/grip-vertical.svg";
import heading from "./icons/heading.svg";
import image from "./icons/image.svg";
import info from "./icons/info.svg";
import italic from "./icons/italic.svg";
import libraryBig from "./icons/library-big.svg";
import link from "./icons/link.svg";
import list from "./icons/list.svg";
import listOrdered from "./icons/list-ordered.svg";
import listPlus from "./icons/list-plus.svg";
import minus from "./icons/minus.svg";
import lock from "./icons/lock.svg";
import lockOpen from "./icons/lock-open.svg";
import palette from "./icons/palette.svg";
import panelRight from "./icons/panel-right.svg";
import pencil from "./icons/pencil.svg";
import plus from "./icons/plus.svg";
import rotateCcw from "./icons/rotate-ccw.svg";
import search from "./icons/search.svg";
import searchX from "./icons/search-x.svg";
import settings from "./icons/settings.svg";
import sigma from "./icons/sigma.svg";
import squareCode from "./icons/square-code.svg";
import stamp from "./icons/stamp.svg";
import table from "./icons/table.svg";
import trash from "./icons/trash-2.svg";
import triangleAlert from "./icons/triangle-alert.svg";
import trophy from "./icons/trophy.svg";
import x from "./icons/x.svg";
import zoomIn from "./icons/zoom-in.svg";
import zoomOut from "./icons/zoom-out.svg";

const ICONS = {
  arrowLeft, arrowRight, bold, bot, check, chevronDown, chevronLeft, chevronRight, minus, chevronsUpDown, circleAlert, code, copy,
  cornerDownLeft, ellipsis, fileDown, filePenLine, fileText, filterX, folder, folderOpen, folderPlus,
  gripVertical, heading, image, info, italic, libraryBig, link, list, listOrdered, listPlus, lock, lockOpen,
  palette, panelRight, pencil, plus, rotateCcw, search, searchX, settings, sigma, squareCode, stamp, table,
  trash, triangleAlert, trophy, x, zoomIn, zoomOut,
};

export type IconName = keyof typeof ICONS;

/** The theme tokens an icon takes its color from. */
export type IconTone =
  | "foreground"
  | "secondary-foreground"
  | "muted-foreground"
  | "primary"
  | "primary-foreground"
  | "destructive"
  | "chart-4";

/** A line icon, 16 points by default, in the secondary text color unless told. */
export function Icon(props: { name: IconName; size?: number; tone?: IconTone; style?: StyleProp<ImageStyle> }) {
  const size = props.size ?? 16;
  return (
    <Image
      src={ICONS[props.name]}
      style={[{ width: size, height: size, tintColor: token(props.tone ?? "secondary-foreground") }, props.style]}
    />
  );
}
