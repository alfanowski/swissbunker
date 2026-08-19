# Fase 2 — Verifica della Forge

**Data:** 2026-08-19
**Hardware:** Apple M4, 16 GB, macOS · volume exFAT
**Esito:** percorso end-to-end **funzionante**; alcune parti pianificate restano fuori, elencate sotto

---

## Sommario

Il percorso completo esiste e gira: un file sul disco diventa un indice, l'indice si apre nel
browser, e c'è un pannello invece di comandi. **57 test Rust** più le verifiche in browser
reale, verdi su APFS e su exFAT.

Il cambio di scope chiesto a metà fase — *"facciamo la custodia, non il contenuto"* — ha
tolto il download manager dal percorso critico e messo al centro l'import. È stato un
guadagno netto: la parte scartata era anche la più lunga, e quella promossa era il pezzo che
mancava davvero.

La scoperta che è costata più tempo non era in nessun piano: **SQLite non riesce a scrivere su
exFAT con le impostazioni di default.**

---

## 1. Cosa funziona, misurato

| | |
|---|---|
| Journal riprendibile | 13 test, verdi su APFS **e** exFAT |
| Costruzione indice, ordine di importanza | 9 test |
| Manifest auto-descrittivo | 8 test |
| Import JSONL | 13 test |
| Daemon e API | 14 test, di cui 5 sul confine di sicurezza |
| **Totale Rust** | **57** |
| Console in browser reale | verificata in entrambe le modalità |
| Indice della Forge letto dal Reader | 6 check di conformità da `file://` |

Percorso reale, eseguito su un volume exFAT:

```
swissbunkerd serve --disk /Volumes/BUNKER
  → POST /api/build {"corpus":"corpus.jsonl","id":"demo"}   {"job":"demo","started":true}
  → GET  /api/state                                          5 documents, 24 KB
  → GET  /api/health    {"ok":true,"corpora":[{"detail":"5 documents, searchable"}]}
```

E la Console, aperta in Chromium: modalità **Connected** rilevata, corpus elencato con
*"5 documents · 24 KB · kept in the order the source file listed them"*, zero errori console.
Aperta da `file://` senza daemon: modalità **Portable**, sola lettura, nessun errore.

---

## 2. SQLite non scrive su exFAT: il problema più grosso della fase

Sintomo: **12 test su 13 passavano su APFS e fallivano su exFAT**, tutti con
`SQLITE_READONLY_DBMOVED` (1032) — *"database file has moved"*.

Causa: SQLite si difende da un file sostituito sotto i piedi confrontando l'inode del file
aperto. **exFAT non ha inode stabili**, quindi il controllo scatta a vuoto a ogni scrittura.

Misurato, su un volume exFAT reale:

| configurazione | esito |
|---|---|
| default | **fallisce** |
| `locking_mode=EXCLUSIVE` | OK |
| `journal_mode=MEMORY` | OK |
| `journal_mode=OFF` | OK |

**Adottato `locking_mode=EXCLUSIVE`**, che ferma il ricontrollo mantenendo il rollback journal
e quindi l'atomicità. Le altre due comprano lo stesso risultato buttando via la sicurezza in
caso di crash — in un journal di crash-recovery sarebbe una contraddizione. Il costo è che un
solo processo per volta può scrivere: il daemon è l'unico scrittore per progetto, quindi qui
non costa nulla.

Ora è il vincolo **V8** nella spec, e vale per ogni file SQLite scritto sul bunker.

**È emerso solo perché il piano prescriveva di rigirare i test su exFAT invece che sul disco
interno.** Su un prodotto che vive su exFAT, scoprirlo in Fase 6 sarebbe stato un disastro.

---

## 3. Confronto con Project Nomad

Esaminato su richiesta. **Nomad non costruisce alcun indice**: orchestra container e delega la
ricerca a Kiwix, che legge l'indice Xapian già presente dentro ogni ZIM.

**Cosa non si applica.** Usare quell'Xapian invece di costruire FTS5 costerebbe zero tempo e
zero spazio, ed è allettante. Scartato per cinque ragioni ora scritte in spec §6.4bis: niente
ricerca trasversale fra corpora, nessun controllo sul ranking, nessuna copertura di PDF e
documenti personali, nessun punteggio BM25 per la fusione della Fase 3, e libzim-in-WASM non
verificato dove `sqlite-wasm` è già misurato.

**Cosa si applica, e ha sbloccato la fase.** Gli ZIM di Kiwix selezionano gli articoli per
popolarità, quindi **l'ordine della sorgente è già un segnale di importanza**. È diventato il
default: gratuito, migliore di casuale, e dichiarato onestamente nel manifest.

---

## 4. Decisioni prese, e le loro ragioni

- **Il confine di sicurezza è l'indirizzo, non un'opzione.** `bind()` rifiuta ogni indirizzo
  non-loopback e la CLI scrive `127.0.0.1` in duro: un flag per allargarlo sarebbe un flag per
  rimuoverlo, su un disco progettato per macchine che non sono dell'utente. Verificato anche
  contro l'IP di rete reale della macchina, che non risponde.
- **I percorsi vengono canonicalizzati e rifiutati se escono dal disco.** Qualsiasi pagina che
  raggiunge loopback può chiedere al daemon di leggere un file.
- **Il progresso è una vista sul journal**, non un canale parallelo. Il journal è già la fonte
  di verità e sopravvive ai crash; un secondo stato divergerebbe proprio quando serve.
- **JSONL come formato nativo**: si produce da shell, sopravvive a un'ultima riga troncata, e
  non carica il corpus in memoria. Un formato che l'operatore sa generare vale più di uno che
  si analizza più in fretta.
- **Una sola codebase per le due modalità della Console.** Due divergerebbero, e la divergenza
  apparirebbe sulla macchina dove non c'è modo di fare debug.
- **`/api/health` apre davvero ogni indice e ci esegue una query**: una voce di manifest il cui
  file è sparito sembra sana guardando solo il manifest.

---

## 5. Cosa NON è stato fatto

1. **Nessun download manager.** Fuori scope per decisione del committente: i contenuti li mette
   lui. Il Task 3 resta scritto nel piano ma non è sul percorso critico.
2. **Nessun lettore ZIM.** Riconosciuto dai magic byte e **rifiutato con istruzioni** invece che
   ignorato. Serve un binding libzim, e la regola del progetto è ispezionare un'API prima di
   scriverci contro — regola pagata tre volte per non averla seguita.
3. **Import in memoria.** `build` carica il corpus in RAM prima di indicizzare. Va bene fino a
   qualche gigabyte di testo; una Wikipedia intera richiede staging su disco. Il journal
   registra già abbastanza per supportarlo.
4. **Build non riprendibile a metà.** Ogni passata è una transazione unica: un'interruzione
   riparte dall'inizio della passata, non dalla posizione nel journal. Accettabile finché una
   passata dura minuti.
5. **Un solo build alla volta**, rifiutando il secondo con un messaggio. Non è una coda.
6. **Nessuna verifica su Windows o Linux.** Il daemon si compila per tutti e tre; solo macOS è
   stato provato. `install.ps1` non esiste ancora.
7. **Nessuna interruzione reale provata.** Il criterio di uscita chiedeva di staccare il cavo
   durante download, estrazione e indicizzazione. Il journal è testato, quel gesto no.
8. **`install.sh` non è mai stato eseguito su un disco vuoto vero.** La sintassi è verificata,
   il percorso completo no.

---

## 6. Cosa serve prima della Fase 3

1. Eseguire `install.sh` davvero, su un disco vuoto, dall'inizio alla fine.
2. Staccare il cavo a metà di una build e verificare che riprenda.
3. Decidere se l'indice diventa contentless — dimezza lo spazio e rende obbligatorio il lettore
   ZIM per gli snippet. È la decisione che stabilisce se quel lettore è sul percorso critico.
4. Provare su Windows, che è metà della promessa "un PC a caso".
