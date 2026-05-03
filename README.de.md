# SC LangPatch

Ein Sprachpaket-*Patcher* für Star Citizen.

🇬🇧 [English version](README.md)

[![Neueste Version](https://img.shields.io/github/v/release/VeeLume/sc-langpatch?display_name=tag)](https://github.com/VeeLume/sc-langpatch/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/VeeLume/sc-langpatch/total)](https://github.com/VeeLume/sc-langpatch/releases)
[![Lizenz: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Die meisten Star-Citizen-Sprachpakete sind eine `global.ini`-Datei, die man von Hand ins Spielverzeichnis kopiert. **SC LangPatch ist der Patcher selbst** — ein kleines Windows-Programm, das die Daten direkt aus *deiner* `Data.p4k` liest und für jede Installation (LIVE / PTU / EPTU / TECH-PREVIEW) eine frische, zur Spielversion passende `global.ini` schreibt. Kein Kopieren von Hand, kein Suchen der richtigen Datei nach jedem Update, kein Warten darauf, dass jemand das Pack neu erstellt — jeder Patch wird aus deinen aktuellen Spieldaten erzeugt. Jede Erweiterung kann einzeln ein- oder ausgeschaltet werden.

> [!IMPORTANT]
> **Du nutzt das deutsche Sprachpaket von [rjcncpt](https://github.com/rjcncpt/StarCitizen-Deutsch-INI)?** Dann lies den Abschnitt [Mit dem deutschen Sprachpaket nutzen](#mit-dem-deutschen-sprachpaket-nutzen) — dafür ist diese Anleitung gedacht. SC LangPatch ersetzt das deutsche Pack nicht, sondern legt seine Erweiterungen *darüber*: deine deutsche Übersetzung bleibt, die Zusatz-Labels (Komponenten-Stufen, illegale Waren, Missions-Infos) kommen oben drauf.

![SC LangPatch Hauptfenster](docs/screenshots/app-main.png)

> [!NOTE]
> Inspiriert von der Sprachpaket-Idee von [ExoAE](https://github.com/ExoAE/ScCompLangPack) und [BeltaKodas Remix](https://github.com/BeltaKoda/ScCompLangPackRemix). Wenn du keinen Patcher laufen lassen willst, sind diese statischen Pakete (und [MrKrakens StarStrings](https://github.com/MrKraken/StarStrings)) hervorragend gepflegt. SC LangPatch macht dieselbe Arbeit *automatisch* gegen die Spielversion, die bei dir gerade installiert ist.

## Was wird ergänzt?

- **Komponenten-Klasse + Stufe** als Präfix, damit du Loot auf einen Blick einschätzen kannst
  - `Bracer` → `MIL1C Bracer`  *(Military, Größe 1, Stufe C)*
  - `XL-1` → `MIL2A XL-1`
- **Markierung illegaler Waren** — basiert auf den Gesetzesdaten der Jurisdiktionen, nicht auf einer handgepflegten Liste
  - `Altruciatoxin` → `[!] Altruciatoxin`
- **Mission Enhancer** — schreibt Missionsbeschreibungen so um, dass sichtbar wird, was das Briefing aktuell verschweigt
  - Bauplan-Belohnungen (mit echten Item-Namen), Reputationsgewinne, Cooldowns, Schiffsbegegnungen
  - Titel-Tags: `[Solo]`, `[Uniq]`, `[BP]` / `[BP*]` / `[BP?]`, `[Illegal]`, `[CS Risk]`
- **Weapon Enhancer** — Größen-Präfixe, Lenkmodus von Raketen und Kampfwerte in den Waffenbeschreibungen
  - `Dominator II Missile` → `[EM] Dominator II Missile`
- **Label- & Key-Fixes** — kürzt Waren-/HUD-Namen, die aus ihrem UI-Feld herauslaufen, und repariert falsch geschriebene Lokalisierungs-Schlüssel
  - `Hephaestanite (Raw)` → `Heph (Raw)`
  - `Instability:` → `Instab.:`

> [!TIP]
> Eine Änderung gefällt dir nicht? Schalte das Modul aus und patche neu. Du willst gar nichts davon? **Patch entfernen** stellt mit einem Klick den Originalzustand wieder her — es wurden nie Spieldateien verändert.

> [!WARNING]
> **Nach jedem Star-Citizen-Update neu patchen.** Die Patches werden aus der aktuell installierten Spielversion abgeleitet. Wenn du einen Re-Patch überspringst, können die Labels auf Schlüssel zeigen, die sich verschoben haben.

---

## Schnellstart

1. **Installer herunterladen** vom [neuesten Release](https://github.com/VeeLume/sc-langpatch/releases/latest) — wähle `SC.LangPatch_X.Y.Z_x64-setup.exe`.
2. **Ausführen.** Windows SmartScreen warnt eventuell, weil die Datei nicht signiert ist — auf *Weitere Informationen* → *Trotzdem ausführen* klicken. Das Programm installiert sich ins übliche Programme-Verzeichnis und legt einen Startmenü-Eintrag an.
3. **SC LangPatch öffnen.** Es erkennt automatisch jede Star-Citizen-Installation auf deinem PC (LIVE, PTU, EPTU, TECH-PREVIEW), indem es die Logdatei des RSI Launchers ausliest. Setze einen Haken bei den Kanälen, die du patchen willst.
4. **Auf "Patch" klicken.** Fertig. Star Citizen starten — die neuen Labels sind sofort da.

Zum Rückgängigmachen jederzeit auf **Patch entfernen** klicken — die erzeugte Datei wird gelöscht und Star Citizen fällt auf seine eingebauten englischen Texte zurück.

---

## Mit dem deutschen Sprachpaket nutzen

Diese Anleitung richtet sich an alle, die das [deutsche Sprachpaket von rjcncpt](https://github.com/rjcncpt/StarCitizen-Deutsch-INI) (oder eine andere Übersetzung) nutzen und die Erweiterungen von SC LangPatch *zusätzlich* haben wollen.

### Wie das zusammenspielt

> [!IMPORTANT]
> SC LangPatch **schreibt seine `global.ini` immer in den `english`-Ordner** und stellt in der `user.cfg` `g_language = english` ein. Das ist gewollt: Die Patches werden gegen die englischen Texte erzeugt, das Spiel muss also auch auf "english" stehen, damit es die gepatchte Datei einliest. Trag deshalb in der `user.cfg` **nicht** `german_(germany)` ein — SC LangPatch würde es bei jedem Patch-Vorgang wieder auf `english` zurücksetzen.
>
> Den deutschen Text bekommst du trotzdem: SC LangPatch nimmt die deutsche `global.ini`, die du im Feld **Sprachpaket** angibst, als Grundlage, legt seine Änderungen oben drauf und legt das Ergebnis in den `english`-Ordner. Das Spiel sieht dann unter "english" eine INI, die in Wirklichkeit deutsch ist — plus Erweiterungen.

Kurz gesagt: Du installierst das deutsche Sprachpaket **nicht** mehr selbst ins Spiel — du gibst SC LangPatch nur die deutsche `global.ini` als Quelle, und SC LangPatch macht den Rest.

### Schritt für Schritt

1. **Du brauchst die deutsche `global.ini` als Quelle.** Zwei einfache Wege:

   **Variante A — direkter Link von GitHub (empfohlen):**
   Diese URL zeigt immer auf die aktuelle Version für LIVE und kann direkt in SC LangPatch eingefügt werden:
   ```
   https://github.com/rjcncpt/StarCitizen-Deutsch-INI/blob/main/live/global.ini
   ```
   Vorteil: nach jedem Spielupdate ist die Datei automatisch aktuell — du musst nichts manuell aktualisieren.
   *(SC LangPatch wandelt GitHub-`blob/`-Links automatisch in den korrekten Download-Link um — du musst dich darum nicht kümmern.)*

   **Variante B — lokale Datei:**
   Wenn du irgendwo eine deutsche `global.ini` auf der Festplatte liegen hast (z. B. aus einer früheren manuellen Installation oder vom SC Deutsch Launcher), kannst du den Pfad direkt eintragen, etwa:
   ```
   C:\…\StarCitizen-Deutsch-INI\live\global.ini
   ```
   In dem Fall musst du die Datei selbst aktuell halten, wenn das Spiel ein Update bekommt.

2. **In SC LangPatch** den Link oder Pfad in das Feld **Sprachpaket** einfügen.

3. **Module aussuchen**, die du dazu haben willst, und auf **Patch** klicken. SC LangPatch lädt die deutsche INI, legt die Module obendrauf, schreibt das Ergebnis in den `english`-Ordner und stellt `g_language = english` in der `user.cfg` ein.

4. **Star Citizen starten** — du siehst jetzt deutsche Texte *plus* die Erweiterungen (Komponenten-Stufen, illegale Waren, Missions-Infos usw.).

### Worauf solltest du achten?

- **`g_language` nicht selbst auf `german_(germany)` setzen.** SC LangPatch verwaltet diese Einstellung und setzt sie bei jedem Patch zurück auf `english`. Das ist korrekt so — die *Datei* im `english`-Ordner enthält dann den deutschen Text.
- **Das deutsche Sprachpaket nicht parallel manuell installieren.** Es wird nicht gebraucht: SC LangPatch erzeugt die einzige `global.ini`, die das Spiel liest. Wenn der SC Deutsch Launcher zusätzlich Dateien in den `german_(germany)`-Ordner legt, schadet das zwar nicht — gelesen wird aber nur der `english`-Ordner.
- **Nach jedem Spielupdate neu patchen.** Bei Variante A reicht das — der Online-Link ist immer aktuell. Bei Variante B vorher die deutsche `global.ini` selbst aktualisieren.
- **Module sind unabhängig.** Wenn dir z. B. einzelne englische Begriffe im deutschen Text stören, kannst du das jeweilige Modul einfach ausschalten und neu patchen.
- **Rückgängig machen.** **Patch entfernen** löscht die gepatchte Datei und setzt `g_language` wieder zurück. Das Spiel ist danach wieder auf Englisch — das deutsche Sprachpaket war nie wirklich "installiert", es wurde ja nur als Vorlage genutzt.

---

## Screenshots

### Das Programm

![Modul-Auswahl](docs/screenshots/app-modules.png)

*Such dir aus, welche Erweiterungen du willst. Jedes Modul ist unabhängig — schalte ein, was nützlich ist, lass den Rest aus.*

![Patch-Ergebnis](docs/screenshots/app-results.png)

*Nach dem Patchen zeigt das Ergebnis-Panel, wie viele Schlüssel jedes Modul geändert hat — und meldet alles, was nicht sauber aufgelöst werden konnte.*

### Im Spiel

![Mission Enhancer im Contract-Terminal](docs/screenshots/ingame-mission.png)

*Mission Enhancer im Contract-Terminal. Das `[BP?]`-Tag im Titel, der **Variants**-Block, die **Blueprints**-Liste (Monde Arms, Monde Core, Monde Helmet usw.) und die **Encounters**-Aufschlüsselung kommen alle aus dem DataCore — nichts davon zeigt das normale Briefing.*

![Komponenten-Grade-Präfix in der Inspect-Ansicht](docs/screenshots/ingame-component-grades.png)

*Komponenten-Grade-Präfix in der Inspect-Ansicht — Klasse/Größe/Stufe stehen vor dem Komponentennamen, damit du Loot auf einen Blick einschätzen kannst.*

---

## FAQ

**Verstößt das gegen die Star-Citizen-AGB?**
SC LangPatch schreibt ausschließlich in die `global.ini` — dieselbe Datei, die CIG im Klartext mitliefert, damit Übersetzer sie bearbeiten können. Es werden keine Programmdateien verändert, kein Spielprozess gehookt und die `Data.p4k` nicht angefasst. Trotzdem: Mods werden offiziell nicht unterstützt — Nutzung auf eigene Gefahr.

**Geht es nach jedem Patch kaputt?**
Die Anpassungen werden live aus deiner aktuellen `Data.p4k` abgeleitet, passen sich also der neuen Spielversion an. Einfach nach jedem Star-Citizen-Update den Patcher noch mal laufen lassen.

**Kann ich es rückgängig machen?**
Ja. Der Knopf **Patch entfernen** löscht die erzeugte Datei und setzt `g_language` in der `user.cfg` wieder zurück. Star Citizen fällt auf seinen eingebauten englischen Text zurück, genau so, als hättest du nie gepatcht.

**Warum warnt Windows SmartScreen?**
Der Installer ist nicht mit einem (kostenpflichtigen) Code-Signing-Zertifikat signiert. Die Builds kommen direkt aus dem [öffentlichen Quellcode](https://github.com/VeeLume/sc-langpatch) und der CI; wer ganz sichergehen will, kann die Signatur über `latest.json` prüfen.

**Es werden keine Installationen gefunden.**
Das Programm findet Installationen, indem es die Zeile `Launching {Version}` aus dem Log des RSI Launchers liest. Jeder Kanal (LIVE, PTU, EPTU, TECH-PREVIEW) taucht erst auf, nachdem du das Spiel aus diesem Kanal mindestens einmal *gestartet* hast — den Launcher zu öffnen reicht nicht. Starte jeden gewünschten Kanal einmal, dann SC LangPatch neu öffnen.

**Im Spiel ist alles auf Englisch, obwohl ich das deutsche Sprachpaket eingetragen habe.**
Das ist tatsächlich erstmal *richtig*: SC LangPatch schreibt die (deutsch befüllte) `global.ini` in den `english`-Ordner und stellt das Spiel auf `english`. Wenn der Text trotzdem englisch *bleibt*, dann konnte SC LangPatch das deutsche Sprachpaket vermutlich nicht laden — schau im Ergebnis-Panel nach Fehlermeldungen zur Sprachpaket-URL/Pfad und prüfe, ob die Datei wirklich erreichbar ist.

**Muss ich `g_language = german_(germany)` in die `user.cfg` schreiben?**
Nein, im Gegenteil — bitte *nicht*. SC LangPatch verwaltet diese Einstellung selbst und setzt sie auf `english`, weil die gepatchte Datei im `english`-Ordner liegt. Trag man `german_(germany)` ein, ignoriert das Spiel die gepatchte Datei und du siehst weder Übersetzung noch Erweiterungen.

---

## Hinweis zu KI-Unterstützung

Teile dieses Codes — und dieses READMEs — wurden mit Hilfe von KI-Werkzeugen geschrieben (vor allem Claude Code). Jede Änderung wird vor dem Einchecken überprüft, und das Projekt hat Tests rund um die Patching-Pipeline. Trotzdem will ich das offen sagen, statt so zu tun, als wäre es anders. Falls dir etwas seltsam vorkommt, sind ein Issue oder ein PR sehr willkommen.

---

## Lizenz

[MIT](LICENSE)
