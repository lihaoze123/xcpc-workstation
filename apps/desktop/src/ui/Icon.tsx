// Lucide vector icons via `nsh-icon` + `nsh-icon-lucide`, tinted with a theme token or custom color.

import { type ColorValue, type StyleProp, type ViewStyle } from "nsh";
import { Icon as NshIcon } from "nsh-icon";
import {
  ArrowLeft,
  ArrowRight,
  Bold,
  BookOpen,
  Bot,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  CircleAlert,
  Code,
  Copy,
  Crown,
  Database,
  Ellipsis,
  FileCode2,
  FileDown,
  FilePenLine,
  FileText,
  FilterX,
  Folder,
  FolderOpen,
  FolderPlus,
  GripVertical,
  Heading,
  Image,
  Info,
  Italic,
  Keyboard,
  LibraryBig,
  Link,
  List,
  ListOrdered,
  ListPlus,
  Lock,
  LockOpen,
  Minus,
  Palette,
  PanelLeft,
  PanelRight,
  Pencil,
  Play,
  Plus,
  RotateCcw,
  Scale,
  Search,
  SearchX,
  Settings,
  ShieldCheck,
  Sigma,
  Sparkles,
  SquareCode,
  Stamp,
  Table,
  Trash2,
  TriangleAlert,
  Trophy,
  X,
} from "nsh-icon-lucide";

const ICONS = {
  arrowLeft: ArrowLeft,
  arrowRight: ArrowRight,
  bold: Bold,
  bookOpen: BookOpen,
  bot: Bot,
  check: Check,
  chevronDown: ChevronDown,
  chevronLeft: ChevronLeft,
  chevronRight: ChevronRight,
  chevronUp: ChevronUp,
  circleAlert: CircleAlert,
  code: Code,
  copy: Copy,
  crown: Crown,
  database: Database,
  ellipsis: Ellipsis,
  fileCode2: FileCode2,
  fileDown: FileDown,
  filePenLine: FilePenLine,
  fileText: FileText,
  filterX: FilterX,
  folder: Folder,
  folderOpen: FolderOpen,
  folderPlus: FolderPlus,
  gripVertical: GripVertical,
  heading: Heading,
  image: Image,
  info: Info,
  italic: Italic,
  keyboard: Keyboard,
  libraryBig: LibraryBig,
  link: Link,
  list: List,
  listOrdered: ListOrdered,
  listPlus: ListPlus,
  lock: Lock,
  lockOpen: LockOpen,
  minus: Minus,
  palette: Palette,
  panelLeft: PanelLeft,
  panelRight: PanelRight,
  pencil: Pencil,
  play: Play,
  plus: Plus,
  rotateCcw: RotateCcw,
  scale: Scale,
  search: Search,
  searchX: SearchX,
  settings: Settings,
  shieldCheck: ShieldCheck,
  sigma: Sigma,
  sparkles: Sparkles,
  squareCode: SquareCode,
  stamp: Stamp,
  table: Table,
  trash: Trash2,
  triangleAlert: TriangleAlert,
  trophy: Trophy,
  x: X,
} as const;

export type IconName = keyof typeof ICONS;

/** The theme tokens an icon takes its color from. */
export type IconTone =
  | "foreground"
  | "secondary-foreground"
  | "muted-foreground"
  | "primary"
  | "primary-foreground"
  | "destructive"
  | "chart-2"
  | "chart-4";

const TONE_CLASS: Record<IconTone, string> = {
  foreground: "text-foreground",
  "secondary-foreground": "text-secondary-foreground",
  "muted-foreground": "text-muted-foreground",
  primary: "text-primary",
  "primary-foreground": "text-primary-foreground",
  destructive: "text-destructive",
  "chart-2": "text-chart-2",
  "chart-4": "text-chart-4",
};

/** A vector line icon, 16 points by default, in the secondary text color unless told. */
export function Icon(props: {
  name: IconName;
  size?: number;
  tone?: IconTone;
  /** Explicit color override when an icon uses a custom palette instead of a theme tone. */
  color?: ColorValue;
  strokeWidth?: number;
  label?: string;
  style?: StyleProp<ViewStyle>;
}) {
  return (
    <NshIcon
      icon={ICONS[props.name]}
      size={props.size ?? 16}
      strokeWidth={props.strokeWidth}
      label={props.label}
      class={props.color !== undefined ? undefined : TONE_CLASS[props.tone ?? "secondary-foreground"]}
      style={props.color !== undefined ? [{ color: props.color } as ViewStyle, props.style] : props.style}
    />
  );
}
