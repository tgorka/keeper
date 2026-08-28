/**
 * The glyphs a space may carry, and how a person finds one (Story 44.4,
 * Story 45.20, FR-198, UX-DR82).
 *
 * **Fixed, and now a browsable set rather than a wall.** 44.4 widened the set
 * from ten to twenty-four and laid them out as one flat wrap of buttons, which
 * is the largest a flat wrap can honestly get: past that, choosing stops being
 * "look at the row" and becomes "read every glyph", and the owner's report said
 * so. So the set is much larger, it is grouped by what a person is filing, and
 * it has a search field over the names.
 *
 * Still a fixed set, for 44.4's reason unchanged: an open picker is a decision
 * the user makes every time and a value keeper then has to validate forever.
 * The keys are lucide's own names, so the value in the file is something a human
 * hand-editing frontmatter can guess, and a name that is not in this map draws
 * {@link SpaceIconFallback} **without keeper rewriting what is on disk**.
 *
 * **Nothing is ever removed from this map.** A stored icon name whose entry
 * disappeared would draw the fallback on a space the user deliberately gave a
 * glyph, and `every icon Story 44.4 shipped is still here` is the test that says
 * so. Adding is free; removing orphans somebody's vault.
 *
 * The load-bearing keys are the seeded defaults' (`keeper-core::notes::default_spaces`):
 * `inbox`, `calendar-days`, `pin`, `video` and — new in 45.20 — `layout-template`,
 * for the Templates space. Rust names them as strings and cannot see this file,
 * so `every seeded default names an icon the picker has` is written on both
 * sides of that boundary.
 */
import {
  Activity,
  Anchor,
  Archive,
  Atom,
  Award,
  Banknote,
  Beaker,
  Bell,
  Bike,
  Blocks,
  Bookmark,
  BookOpen,
  Bot,
  Box,
  Boxes,
  Brain,
  Briefcase,
  Bug,
  Building2,
  Cake,
  CalendarCheck,
  CalendarClock,
  CalendarDays,
  Camera,
  Car,
  Cat,
  ChartLine,
  ChartNoAxesColumn,
  CircleCheck,
  ClipboardList,
  Clock,
  Cloud,
  Code,
  Coffee,
  Compass,
  Component,
  Contact,
  Cpu,
  CreditCard,
  Crown,
  Database,
  Diamond,
  Dog,
  Droplet,
  Dumbbell,
  Eye,
  Feather,
  FileStack,
  Files,
  FileText,
  Film,
  Filter,
  Fingerprint,
  Flag,
  Flame,
  FlaskConical,
  Flower,
  Folder,
  FolderKanban,
  FolderOpen,
  FolderSync,
  FolderTree,
  Footprints,
  Gamepad2,
  Gauge,
  Gem,
  Ghost,
  Gift,
  GitBranch,
  Glasses,
  Globe,
  GraduationCap,
  Guitar,
  Hammer,
  Handshake,
  Hash,
  Headphones,
  Heart,
  Hourglass,
  House,
  Image,
  Inbox,
  // biome-ignore lint/suspicious/noShadowRestrictedNames: lucide-react's own export name, and this module IS the icon-name -> component lookup
  Infinity,
  Key,
  KeyRound,
  Lamp,
  Landmark,
  Laptop,
  Layers,
  LayoutGrid,
  LayoutTemplate,
  Leaf,
  Library,
  Lightbulb,
  Link,
  List,
  ListChecks,
  ListTodo,
  Lock,
  type LucideIcon,
  Luggage,
  Magnet,
  Mail,
  // biome-ignore lint/suspicious/noShadowRestrictedNames: lucide-react's own export name, and this module IS the icon-name -> component lookup
  Map,
  MapPin,
  Medal,
  Megaphone,
  Mic,
  Microscope,
  Milestone,
  Monitor,
  Moon,
  Mountain,
  Music,
  Newspaper,
  Notebook,
  NotebookPen,
  Orbit,
  Package,
  Palette,
  Paperclip,
  PartyPopper,
  Pencil,
  PenTool,
  Phone,
  PieChart,
  Pin,
  Pizza,
  Plane,
  Plug,
  Presentation,
  Puzzle,
  Quote,
  Rabbit,
  Radar,
  Radio,
  Receipt,
  Recycle,
  Repeat,
  Ribbon,
  Rocket,
  Route,
  Ruler,
  Sailboat,
  Satellite,
  Scale,
  Scissors,
  ScrollText,
  Search,
  Send,
  Server,
  Settings,
  Shapes,
  Shield,
  Ship,
  ShoppingBag,
  Shuffle,
  Siren,
  Skull,
  Smile,
  Snowflake,
  Sparkles,
  Sprout,
  Stamp,
  Star,
  Stethoscope,
  StickyNote,
  Store,
  Sun,
  Sword,
  Table,
  Tag,
  Target,
  Telescope,
  TentTree,
  Terminal,
  Thermometer,
  ThumbsUp,
  Ticket,
  Timer,
  Train,
  Trees,
  TrendingUp,
  TriangleAlert,
  Trophy,
  Truck,
  Turtle,
  Umbrella,
  User,
  UserRound,
  Users,
  UsersRound,
  Utensils,
  Vault,
  Video,
  Wallet,
  Wand,
  Watch,
  Waves,
  Webhook,
  Wifi,
  Wind,
  Workflow,
  Wrench,
  Zap,
} from "lucide-react";

/** One browsable section of the picker. */
export interface SpaceIconGroup {
  /** What the section is called, and the accessible name of its grid. */
  readonly label: string;
  /** The section's glyphs, keyed by the name stored in frontmatter. */
  readonly icons: Readonly<Record<string, LucideIcon>>;
}

/**
 * The set, grouped the way a person choosing one is thinking.
 *
 * The grouping is the browsing: "somewhere in these eight rows" is a scan, and
 * "somewhere in these hundred and seventy" is a search. Order within a group is
 * rough usefulness rather than alphabetical, because alphabetical order over
 * glyph names is an order nobody is reading in.
 *
 * The first group is keeper's own vocabulary and holds every key the seeded
 * defaults ask for, so the icons that appear in a fresh vault's rail are the
 * first ones the picker offers.
 */
export const SPACE_ICON_GROUPS: readonly SpaceIconGroup[] = [
  {
    label: "keeper",
    icons: {
      inbox: Inbox,
      "calendar-days": CalendarDays,
      pin: Pin,
      video: Video,
      "layout-template": LayoutTemplate,
      "notebook-pen": NotebookPen,
      "sticky-note": StickyNote,
      "file-text": FileText,
      "file-stack": FileStack,
      "scroll-text": ScrollText,
      "clipboard-list": ClipboardList,
      archive: Archive,
      folder: Folder,
      "folder-tree": FolderTree,
      "folder-sync": FolderSync,
      tag: Tag,
      hash: Hash,
      search: Search,
      filter: Filter,
      layers: Layers,
      "layout-grid": LayoutGrid,
      list: List,
      table: Table,
      quote: Quote,
      link: Link,
      paperclip: Paperclip,
      bookmark: Bookmark,
      star: Star,
      clock: Clock,
      "calendar-clock": CalendarClock,
      mic: Mic,
      radio: Radio,
      film: Film,
      camera: Camera,
      "folder-kanban": FolderKanban,
      "folder-open": FolderOpen,
      files: Files,
      "list-todo": ListTodo,
      "list-checks": ListChecks,
      notebook: Notebook,
      settings: Settings,
    },
  },
  {
    label: "Work",
    icons: {
      briefcase: Briefcase,
      building: Building2,
      handshake: Handshake,
      users: Users,
      contact: Contact,
      presentation: Presentation,
      target: Target,
      milestone: Milestone,
      workflow: Workflow,
      "trending-up": TrendingUp,
      "chart-no-axes-column": ChartNoAxesColumn,
      "pie-chart": PieChart,
      gauge: Gauge,
      scale: Scale,
      receipt: Receipt,
      banknote: Banknote,
      wallet: Wallet,
      "credit-card": CreditCard,
      stamp: Stamp,
      mail: Mail,
      send: Send,
      phone: Phone,
      megaphone: Megaphone,
      newspaper: Newspaper,
      store: Store,
      landmark: Landmark,
      truck: Truck,
      "shopping-bag": ShoppingBag,
      "building-2": Building2,
      user: User,
      "user-round": UserRound,
      "users-round": UsersRound,
    },
  },
  {
    label: "Making",
    icons: {
      code: Code,
      terminal: Terminal,
      cpu: Cpu,
      plug: Plug,
      database: Database,
      server: Server,
      "git-branch": GitBranch,
      webhook: Webhook,
      bug: Bug,
      wrench: Wrench,
      hammer: Hammer,
      ruler: Ruler,
      scissors: Scissors,
      puzzle: Puzzle,
      blocks: Blocks,
      component: Component,
      shapes: Shapes,
      box: Box,
      boxes: Boxes,
      package: Package,
      laptop: Laptop,
      monitor: Monitor,
      bot: Bot,
      palette: Palette,
      "pen-tool": PenTool,
      pencil: Pencil,
      image: Image,
      music: Music,
      headphones: Headphones,
      guitar: Guitar,
      wand: Wand,
    },
  },
  {
    label: "Study",
    icons: {
      book: BookOpen,
      library: Library,
      "graduation-cap": GraduationCap,
      brain: Brain,
      lightbulb: Lightbulb,
      beaker: Beaker,
      "flask-conical": FlaskConical,
      microscope: Microscope,
      telescope: Telescope,
      atom: Atom,
      orbit: Orbit,
      satellite: Satellite,
      radar: Radar,
      infinity: Infinity,
      hourglass: Hourglass,
      timer: Timer,
      thermometer: Thermometer,
      magnet: Magnet,
      globe: Globe,
      map: Map,
      compass: Compass,
      stethoscope: Stethoscope,
      activity: Activity,
      eye: Eye,
      fingerprint: Fingerprint,
      "calendar-check": CalendarCheck,
      "chart-line": ChartLine,
      route: Route,
    },
  },
  {
    label: "Life",
    icons: {
      heart: Heart,
      house: House,
      coffee: Coffee,
      utensils: Utensils,
      pizza: Pizza,
      cake: Cake,
      dumbbell: Dumbbell,
      bike: Bike,
      car: Car,
      train: Train,
      plane: Plane,
      sailboat: Sailboat,
      ship: Ship,
      luggage: Luggage,
      "map-pin": MapPin,
      mountain: Mountain,
      "tent-tree": TentTree,
      trees: Trees,
      leaf: Leaf,
      sprout: Sprout,
      flower: Flower,
      sun: Sun,
      moon: Moon,
      cloud: Cloud,
      snowflake: Snowflake,
      droplet: Droplet,
      waves: Waves,
      wind: Wind,
      flame: Flame,
      umbrella: Umbrella,
      dog: Dog,
      cat: Cat,
      rabbit: Rabbit,
      turtle: Turtle,
      glasses: Glasses,
      watch: Watch,
      lamp: Lamp,
      "party-popper": PartyPopper,
      gift: Gift,
      ticket: Ticket,
      "gamepad-2": Gamepad2,
    },
  },
  {
    label: "Marks",
    icons: {
      flag: Flag,
      bell: Bell,
      zap: Zap,
      sparkles: Sparkles,
      rocket: Rocket,
      crown: Crown,
      trophy: Trophy,
      medal: Medal,
      award: Award,
      ribbon: Ribbon,
      gem: Gem,
      diamond: Diamond,
      key: Key,
      lock: Lock,
      shield: Shield,
      vault: Vault,
      siren: Siren,
      anchor: Anchor,
      feather: Feather,
      recycle: Recycle,
      repeat: Repeat,
      shuffle: Shuffle,
      wifi: Wifi,
      "thumbs-up": ThumbsUp,
      smile: Smile,
      ghost: Ghost,
      skull: Skull,
      sword: Sword,
      footprints: Footprints,
      "circle-check": CircleCheck,
      "triangle-alert": TriangleAlert,
      "key-round": KeyRound,
    },
  },
];

/**
 * Every glyph, flattened, keyed by the name stored in frontmatter.
 *
 * **Derived from the groups rather than maintained beside them.** Two hand-kept
 * lists that must agree is the shape where an icon is browsable and unstorable,
 * or stored and undrawable, and neither failure shows up until somebody picks
 * that one glyph.
 */
export const SPACE_ICONS: Readonly<Record<string, LucideIcon>> = Object.fromEntries(
  SPACE_ICON_GROUPS.flatMap((group) => Object.entries(group.icons)),
);

/**
 * What a space draws when it has no icon — and what it draws when its stored
 * icon is not in {@link SPACE_ICONS}.
 *
 * The unknown case renders this rather than nothing, because a row with a hole
 * where every sibling has a glyph reads as a broken space rather than as an
 * unfamiliar icon name. The *stored value* is untouched: the picker simply shows
 * nothing selected, and saving without choosing sends the name straight back. An
 * icon set changing must not silently rewrite what is in someone's vault, for
 * the same reason a query term keeper cannot parse is not rewritten either.
 */
export const SpaceIconFallback: LucideIcon = Layers;

/** The glyph a space's stored icon name draws. */
export function spaceIcon(name: string | null): LucideIcon {
  return (name !== null ? SPACE_ICONS[name] : undefined) ?? SpaceIconFallback;
}

/**
 * The words that find an icon whose lucide name is not the word a person types.
 *
 * Deliberately short. Every entry is a case where the key genuinely fails the
 * search — "meeting" finds nothing but is what someone filing meetings types —
 * and not a thesaurus, because an alias list that grows without a rule becomes a
 * second naming scheme for the same glyphs and nobody can predict what matches.
 * A key that already contains the obvious word is absent from here on purpose.
 */
const SPACE_ICON_ALIASES: Readonly<Record<string, readonly string[]>> = {
  "layout-template": ["template", "scaffold", "boilerplate", "form"],
  "clipboard-list": ["todo", "task", "checklist"],
  "sticky-note": ["note", "memo"],
  "notebook-pen": ["note", "journal", "diary", "write"],
  "calendar-days": ["date", "schedule", "month"],
  "calendar-clock": ["deadline", "due", "reminder"],
  users: ["people", "team", "meeting", "group"],
  contact: ["person", "card", "address"],
  banknote: ["money", "cash", "budget", "finance"],
  wallet: ["money", "budget", "finance", "spending"],
  "trending-up": ["growth", "metrics", "stats"],
  "chart-no-axes-column": ["chart", "graph", "stats", "metrics"],
  "pie-chart": ["chart", "graph", "stats"],
  house: ["home", "personal"],
  "flask-conical": ["experiment", "lab", "science"],
  beaker: ["experiment", "lab", "science"],
  "graduation-cap": ["school", "study", "learning", "course"],
  bug: ["defect", "issue", "problem"],
  "git-branch": ["git", "version", "branch", "repo"],
  vault: ["safe", "secure", "secret"],
  zap: ["fast", "quick", "energy", "urgent"],
  radio: ["network", "broadcast", "signal"],
  film: ["recording", "movie", "video"],
  mic: ["recording", "audio", "voice", "podcast"],
  "map-pin": ["place", "location", "travel"],
  "gamepad-2": ["game", "play", "gaming"],
};

/**
 * The groups, filtered to the icons whose name matches `query`.
 *
 * Pure over the query and the catalogue, so what "finds" means is assertable
 * without a dialog. Empty groups are dropped, so the chooser renders only
 * sections that have something in them — a heading over nothing reads as a
 * broken filter.
 *
 * Matching is substring, case-insensitive, and hyphen-insensitive in both
 * directions: `layout template`, `layout-template` and `TEMPLATE` are one
 * search. Hyphens are how lucide spells a space and nobody types them.
 *
 * A blank query is every group, unfiltered — the browsable state, which is the
 * one the chooser opens in.
 */
export function matchSpaceIcons(query: string): readonly SpaceIconGroup[] {
  const needle = query.trim().toLowerCase().replace(/-/g, " ");
  if (needle === "") {
    return SPACE_ICON_GROUPS;
  }
  const matches: SpaceIconGroup[] = [];
  for (const group of SPACE_ICON_GROUPS) {
    const icons = Object.entries(group.icons).filter(
      ([key]) =>
        key.replace(/-/g, " ").includes(needle) ||
        (SPACE_ICON_ALIASES[key] ?? []).some((alias) => alias.includes(needle)),
    );
    if (icons.length > 0) {
      matches.push({ label: group.label, icons: Object.fromEntries(icons) });
    }
  }
  return matches;
}
