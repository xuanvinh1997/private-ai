import { LOCALES, locale, S, setLocale, t, type Locale } from "../../lib/i18n";
import { displayMode, setDisplayMode, type DisplayMode } from "../../lib/prefs";
import { setTheme, theme, type ThemeChoice } from "../../lib/theme";
import { Row, RowGroup, SectionHead, Select } from "./FormKit";

/** The General page: the only three settings that do not touch the core, each a row with a select on the right, matching every other settings page. */

/** Each language's name in its own language, never a `Msg`, so someone stuck in a language they cannot read still recognises the way back. */
const TEN_NGON_NGU: Record<Locale, string> = {
  en: "English",
  vi: "Tiếng Việt",
};

export default function GeneralPage() {
  return (
    <div class="flex flex-col gap-2xl">
      <section class="flex flex-col gap-md">
        <SectionHead
          icon="eye"
          title={t(S.settings.general.displayTitle)}
          desc={t(S.settings.general.displayDesc)}
        />
        <RowGroup>
          <Row
            icon="palette"
            label={t(S.settings.general.theme)}
            desc={t(S.settings.general.themeDesc)}
            // more={t(S.settings.general.themeMore)}
            control={() => (
              <Select
                label={t(S.settings.general.theme)}
                value={theme()}
                onPick={(value) => setTheme(value as ThemeChoice)}
                options={[
                  { id: "light", label: t(S.settings.general.themeLight) },
                  { id: "dark", label: t(S.settings.general.themeDark) },
                  { id: "system", label: t(S.settings.general.themeSystem) },
                ]}
              />
            )}
          />
          <Row
            icon="globe"
            label={t(S.settings.general.locale)}
            desc={t(S.settings.general.localeDesc)}
            // more={t(S.settings.general.localeMore)}
            control={() => (
              <Select
                label={t(S.settings.general.locale)}
                value={locale()}
                onPick={(value) => setLocale(value as Locale)}
                options={LOCALES.map((ma) => ({ id: ma, label: TEN_NGON_NGU[ma] }))}
              />
            )}
          />
          <Row
            icon="bubble"
            label={t(S.settings.general.layout)}
            desc={t(S.settings.general.layoutDesc)}
            // more={t(S.settings.general.layoutMore)}
            control={() => (
              <Select
                label={t(S.settings.general.layout)}
                value={displayMode()}
                onPick={(value) => setDisplayMode(value as DisplayMode)}
                options={[
                  { id: "bubble", label: t(S.settings.general.layoutBubble) },
                  { id: "document", label: t(S.settings.general.layoutDocument) },
                ]}
              />
            )}
          />
        </RowGroup>
      </section>
    </div>
  );
}
