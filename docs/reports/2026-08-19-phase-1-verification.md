# Fase 1 — Verifica del Reader

**Data:** 2026-08-19
**Hardware:** Apple M4, 16 GB, macOS · immagine exFAT sparse
**Motore:** `@sqlite.org/sqlite-wasm` 3.53.0 · Chromium via Playwright, headed, zero flag permissivi
**Esito:** criterio di latenza **superato**; criterio su contenuto reale **non verificato**

---

## Sommario

Il Reader funziona. Da una pagina aperta con `file://`, senza nulla installato sulla macchina,
un indice SQLite FTS5 da **6.22 GB** si apre in **19 ms** e risponde a una ricerca in
**17.3 ms**, leggendo lo **0.014%** del file. Il criterio di uscita chiedeva meno di 500 ms:
il margine è di trenta volte.

Il percorso non è stato liscio, e le due cose che l'hanno quasi fatto fallire non erano nel
piano. Una era un errore di inizializzazione che si manifesta **solo** sotto origin nullo. La
seconda era un collasso di prestazioni su termini frequenti, che ha richiesto quattro
esperimenti per essere isolato e ha smentito tre ipotesi mie prima di cedere.

45 test unitari e 7 check di conformità, tutti verdi.

---

## 1. Risultati misurati

Su `fts-test.sqlite`: 6.22 GB, 2 milioni di documenti, FTS5 con contenuto.

| Misura | Valore | Criterio | Margine |
|---|---|---|---|
| Apertura del database | **19 ms** | — | — |
| Ricerca a freddo (termine raro) | **17.3 ms** | < 500 ms | 29× |
| Ricerca a caldo, p50 | **0.1 ms** | — | — |
| Ricerca su termini vari, p95 | **30 ms** | < 500 ms | 17× |
| Termine caldo, 35.575 match | **1.8 ms** | < 500 ms | 278× |
| Frazione di file letta | **0.014%** — 851 KB su 6.22 GB | < 5% | 357× |
| Cache in uso a fine sessione | 7 MB | < 128 MB | — |
| Bundle | 1.31 MB | < 2 MB (§9.1) | — |

Il numero che descrive l'architettura meglio di tutti resta la frazione letta: **851 KB su
6.22 GB**. Non è un'ottimizzazione, è la ragione per cui la modalità Portable esiste.

---

## 2. I due problemi che sono costati il grosso del lavoro

### 2.1 L'inizializzazione moriva solo sotto `file://`

Sintomo: `Failed to construct 'URL': Invalid URL`, in produzione, con tutti i test unitari
verdi.

Causa, trovata leggendo il bundle:

```js
function ag(){ return s.locateFile ? MA("sqlite3.wasm") : new URL("sqlite3.wasm", …).href }
```

Emscripten risolve il **nome** del file wasm prima di guardare `wasmBinary`. In un bundle IIFE
`import.meta.url` collassa in qualcosa che non è un URL valido, quindi il ramo `else` lancia
e l'inizializzazione muore prima ancora di arrivare ai byte già inlineati. Fornire
`locateFile` prende l'altro ramo e il problema sparisce.

**Perché i test unitari non l'hanno visto:** girano su `http://`, dove quell'URL si risolve
benissimo. È esattamente la ragione per cui la suite di conformità è separata, e la prima
volta che quella separazione si è ripagata.

### 2.2 Ordinare per rilevanza costa 164× su termini frequenti

Sintomo: p95 di **4328 ms** su ricerche variate, con 5994 letture, 392 MB e 3946 evictions
contro appena 103 hit di cache — thrashing conclamato.

Sono servite quattro misure, e le prime tre hanno smentito altrettante mie ipotesi:

| Ipotesi | Esito |
|---|---|
| La pagina da 64 KB amplifica le letture | **Sbagliata in parte.** Il numero di letture resta ~6000 a ogni dimensione di pagina; cambiano solo i byte, da 25 MB a 392 MB. Utile ma non la causa |
| Le virgolette della sanitisation forzano un percorso lento | **Sbagliata.** Frase 30 ms, termine nudo 19 ms, con bm25 16 ms, tutte con 40 letture |
| Il costo è nella prima chiamata o in `snippet()` | **Sbagliata.** Ogni variante di query costa 1-12 ms con 3-8 letture |

La causa vera era un solo termine su venti: **`tau`, presente in 35.575 documenti**. Tutti gli
altri non esistevano nel corpus.

| Query su `tau` | Tempo | Letture |
|---|---|---|
| `ORDER BY bm25(docs)` | 2120 ms | 5984 |
| `ORDER BY rank` | **2059 ms** — nessun guadagno | 5956 |
| nessun ordinamento | **12.9 ms** | **39** |
| `count(*)` | — | 41 |

**`ORDER BY rank` non aiuta.** La documentazione FTS5 presenta `rank` come la forma
ottimizzabile, ma per ordinare va comunque percorsa l'intera doclist: il costo è nel dover
assegnare un punteggio a 35.575 documenti per poi buttarne via 35.555. È un limite di design,
non un errore di scrittura della query.

**La mitigazione** sfrutta il fatto che `count(*)` costa 41 letture: si conta prima, e se i
match superano `DEFAULT_RANK_LIMIT` (2000) si restituiscono i risultati in ordine di
archiviazione dichiarando `ranked: false`.

| | prima | dopo |
|---|---|---|
| Termine caldo | 2120 ms | **1.8 ms** |
| Ricerche variate, p95 | 4328 ms | **30 ms** |
| Letture totali | 5994 | **108** |
| MB letti | 392 | **7** |
| Evictions | 3938 | **0** |

`ranked` è esposto fino alla UI, che scrive "35.575 risultati — troppo comune per
ordinarli, mostro i primi 20". Un risultato non ordinato significa *"venti documenti che
contengono questa parola"*, non *"i venti migliori"*: presentarli allo stesso modo
ingannerebbe qualcuno che non ha modo di verificare.

---

## 3. Scelte tarate sulle misure

- **Pagina da 8 KB.** Sweep su 4/8/16/32/64 KB: 8192 vince sul tempo totale (2175 ms contro
  2439 a 4 KB e 4259 a 64 KB). Oltre i 16 KB compaiono le evictions e l'amplificazione domina.
- **Cutoff di ranking a 2000 match**, dal costo misurato sopra.
- **Lettore sequenziale**, come stabilito in Fase 0: il parallelismo rendeva 1.23×.

---

## 4. Cosa NON è stato dimostrato

1. **Nessuna verifica su Wikipedia italiana reale.** Il corpus è sintetico: due milioni di
   documenti con vocabolario casuale e distribuzione **uniforme**. Il linguaggio naturale è
   zipfiano — poche parole comunissime e una lunga coda di rare — quindi la distribuzione
   reale dei termini caldi sarà **diversa**, e il cutoff a 2000 va ritarato su testo vero.
   Questo era il Task 8 del piano e resta aperto.
2. **Un solo motore.** Solo Chromium. Firefox e WebKit non sono stati rieseguiti dopo la
   Fase 0, e la correzione di `locateFile` non è stata verificata su di loro.
3. **Nessun disco USB reale.** Le latenze vengono da un'immagine exFAT su NVMe interno. La
   XHR sincrona è misurata a 0.7 ms per 4 KB; su USB sarà peggiore, e con ~108 letture per
   sessione il conto va rifatto.
4. **Solo macOS.** Windows non è stato toccato.
5. **Nessun test di concorrenza.** Due database aperti insieme funzionano; niente è stato
   provato sotto ricerche simultanee.
6. **Nessun lettore ZIM.** Spostato in Fase 2 per decisione esplicita: l'indice di questa
   fase conserva il testo, quindi i risultati sono verificabili senza.

---

## 4bis. Ritaraggio su italiano reale (2026-08-19)

Il limite principale della §4 è stato affrontato: 1.431 incipit casuali di Wikipedia IT presi
via API, più i 330 articoli più visitati, contro un set di 22 query che una persona
digiterebbe davvero.

### La distribuzione reale rende il fallback la norma

| Query | Match su 813 doc | % del corpus | Stima su 1.9M articoli |
|---|---|---|---|
| `città` | 75 | 9.2% | **175.000** |
| `guerra` | 52 | 6.4% | **121.000** |
| `storia` | 43 | 5.3% | **100.000** |
| `anno` | 35 | 4.3% | 81.000 |
| `fotosintesi` | 0 | 0% | — |

**Undici query su ventidue supererebbero il cutoff di 2000** su una Wikipedia completa. Il
cutoff resta corretto rispetto al costo — ordinare 175.000 match costerebbe una decina di
secondi — ma la conseguenza è che **il percorso non ordinato diventa quello principale**, non
l'eccezione prevista.

### Conseguenza: l'ordine di archiviazione va reso significativo

Verificato che FTS5, senza `ORDER BY`, restituisce i match in ordine di rowid crescente, in
modo stabile anche con `OFFSET`. Quindi l'ordine di inserimento decide cosa vede l'utente in
metà delle ricerche.

A parità di documenti, cambiando solo l'ordine di inserimento:

| Query | Ordine arbitrario | Ordine per visite |
|---|---|---|
| `guerra` | Mudang, House of the Dragon, Livellatori | **Isole Falkland, Guerra delle Falkland** |
| `città` | Muskegon, San Michele Extra, Provincia di Catania | Campionato mondiale, Argentina |
| `storia` | Corte di Artù, Paganesimo, Gianni Versace | Lionel Messi, Campionato mondiale |

Il requisito è finito nella spec come §6.5. **Costo a query time: zero.**

### Riserve su questa misura

- **Il campione di popolarità è di un solo giorno**, e infatti i risultati pendono verso lo
  sport del momento. Dimostra il meccanismo, non il segnale definitivo: la Forge deve usare
  pageviews aggregate o link entranti.
- **La lunghezza dell'articolo come proxy di importanza è stata provata e non funziona**:
  ordinando per lunghezza i risultati non sono migliori di quelli arbitrari.
- 1.431 incipit sono sufficienti a misurare la forma della distribuzione, non a stimare con
  precisione i conteggi su 1.9M articoli. Le cifre scalate sono ordini di grandezza.
- Gli incipit, non gli articoli interi: l'API limita gli estratti completi a una pagina per
  richiesta. È comunque il testo che la spec §6.2 indicizza nel ramo denso.

---

## 5. Cosa serve prima della Fase 2

1. ~~Costruire un indice da Wikipedia italiana reale e ritarare il cutoff~~ — **fatto**,
   vedi §4bis. Ne è uscito un requisito nuovo per la Forge (spec §6.5), non solo un numero.
2. Rieseguire la conformità su Firefox e WebKit, per la correzione `locateFile`.
3. Rieseguire le misure su un disco USB reale.
4. Decidere se l'indice della Forge sarà contentless: dimezza lo spazio ma rende obbligatorio
   il lettore ZIM per gli snippet. È la decisione che stabilisce se quel lettore è sul
   percorso critico.
