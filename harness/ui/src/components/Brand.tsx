/** Brand mark: a shield with a spark cut out of it, saying the two things the product sells (data stays local,
 * a model inside). One path, `evenodd`, `currentColor`, so the spark is a hole and always matches its backdrop. */
export function BrandMark(props: { size?: number; class?: string }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      width={props.size ?? 24}
      height={props.size ?? 24}
      fill="currentColor"
      fill-rule="evenodd"
      class={props.class}
    >
      <path d="M12 2.5 4.5 5.4V11.5c0 4.5 3 8.6 7.5 9.9 4.5-1.3 7.5-5.4 7.5-9.9V5.4ZM12 6.8l1.15 3.05L16.2 11l-3.05 1.15L12 15.2l-1.15-3.05L7.8 11l3.05-1.15Z" />
    </svg>
  );
}

/** Shield plus name, one component because it appears in two very different layouts; only the shield takes the accent. */
export function BrandLockup(props: { class?: string }) {
  return (
    <span class={`flex min-w-0 items-center gap-xs ${props.class ?? ""}`}>
      <BrandMark size={22} class="shrink-0 text-accent" />
      <span class="min-w-0 truncate text-base font-bold tracking-[-0.01em] text-ink">
        Private AI
      </span>
    </span>
  );
}
