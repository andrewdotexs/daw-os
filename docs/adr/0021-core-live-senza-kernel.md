# ADR 0021 — Core live senza kernel

- **Stato**: accettata dall'utente il 19 novembre 2026, con tre integrazioni proposte dall'architetto e accolte lo stesso giorno (punti 3, 4 e 6). È un principio di progetto per la Fase 1: nell'immagine non c'è ancora nulla che lo realizzi. Precisata il 21 novembre 2026 all'apertura della Fase 1: chi sveglia i thread e la regola sulle chiamate di sistema (in fondo)
- **Data**: 19 novembre 2026
- **Origine**: documento di progetto, **ADR-038** (nel repository la numerazione è un'altra: vedi la [tabella di corrispondenza](README.md)), scheda «Strato 1 — Kernel», sezione «Riscrivere o no: firmware, kernel e driver»; nota dell'utente del 19 novembre 2026; risultati della campagna [P0.7](../fasi/fase-0/P0.7-campagna-di-misura.md)

## Contesto

La Fase 0 ha portato i core live a un ritardo di risveglio medio di 2 µs e massimo di circa 21 µs in 24 ore sotto carico. Ha anche mostrato da dove vengono i disturbi che restano: dal firmware (nessun SMI, ma solo finché le sue impostazioni non cambiano), dal kernel (interrupt tra core quando riscrive il proprio codice o pulisce le tabelle delle pagine), dai driver (quello della scheda audio è l'unico nel percorso real-time).

L'utente ha valutato se riscrivere, in tutto o in parte, firmware, kernel e driver. La risposta è no per tutti e tre, con un principio comune al loro posto: invece di rendere il kernel più veloce sui core live, **tenerlo fuori dai core live** per tutto il tempo in cui il motore calcola.

## Decisione

**Durante una sessione, nella fase di calcolo del ciclo, i thread del motore sui core live non entrano mai nel kernel e non vengono interrotti da niente.**

### 1. Chi contribuisce, e come

| Dominio sotto la DAW | Contributo | Punto aperto |
| --- | --- | --- |
| Firmware | Nessun SMI: garantito e verificato a ogni avvio | A-48 (Q-12 nel documento di progetto) |
| Kernel | Gli interrupt tra core che restano vengono rinviati al successivo ingresso nel kernel | A-49 (Q-13) |
| Driver audio | Il motore legge e scrive il buffer della scheda in memoria condivisa, senza syscall | A-50 (Q-14) |

### 2. Il confine del ciclo

Il ciclo ha due fasi.

1. **Calcolo**: dal risveglio alla consegna del periodo. Il motore resta in spazio utente puro: niente syscall, niente futex, niente allocazioni che possano chiamare il kernel, niente I/O.
2. **Attesa**: tra la consegna del periodo e il risveglio successivo. Qui è ammessa al massimo una chiamata bloccante per periodo, più l'eventuale notifica del puntatore al driver audio (A-50).

**Perché due fasi e non zero syscall in assoluto.** Gli interrupt dannosi sono quelli che cadono durante il calcolo, perché allungano il tempo verso la scadenza. Uno che arriva mentre il core aspetta costa al più un piccolo ritardo del risveglio. Il rinvio degli interrupt tra core (A-49) agisce proprio così: rinvia quando il core è in spazio utente e consegna subito quando è nel kernel.

### 3. Le fasi valgono per ogni thread

_Integrazione dell'architetto, accolta._ Con quattro core live il motore ha più thread, e il modo più ovvio di coordinarli, un thread che ne sveglia un altro, è una syscall dentro il calcolo.

- Le due fasi sono di **ogni thread** live, non del motore nel suo insieme.
- **Dentro il calcolo nessun thread ne sveglia un altro.** Tutti i thread live si svegliano da soli all'inizio del periodo, ciascuno con la propria chiamata bloccante.
- Il lavoro passa da un thread all'altro con **code senza lock** e, dove serve aspettare, con attesa attiva di durata limitata.
- Un thread che finisce prima può entrare nella propria attesa; **non può essere richiamato nello stesso ciclo**.

È un vincolo sul modo di distribuire il grafo tra i core live: la ripartizione si decide prima del ciclo, oppure i thread si passano il lavoro mentre sono tutti svegli. I thread del dominio anticipativo non sono toccati da questa decisione.

### 4. L'attesa è il punto di cessione, e il motore la garantisce

L'attesa è anche il momento in cui il motore cede il core, come richiede la protezione del rischio K-04: con il _fair server_ spento sui core live ([ADR 0018](0018-profili-di-avvio-e-regolazione.md)), i thread del kernel legati a un core girano solo quando il thread real-time si ferma. **Un ciclo in attesa attiva pura, senza chiamata bloccante, è escluso** per questo motivo.

_Integrazione dell'architetto, accolta._ L'attesa cede il core solo se il thread dorme davvero. Se un ciclo sfora il periodo, dormire fino a un istante già passato ritorna subito: una sessione troppo pesante, in sforamento continuo, lascerebbe il kernel senza tempo come un thread impazzito.

- **Bilancio di calcolo**: il motore smette di calcolare a un margine fisso prima della fine del periodo. Il ciclo incompleto è un difetto, contato con gli altri, e l'attesa reale c'è in ogni ciclo.
- **Il margine si misura**: è il tempo che serve ai thread del kernel di un core live per fare il loro lavoro _(da misurare, Fase 1)_.
- **Il sorvegliante esterno** del punto aperto A-18 resta come seconda linea, per il thread che non ritorna affatto: un plugin bloccato in un ciclo senza fine non rispetta nessun bilancio.

### 5. Conseguenze per il progetto del motore

- Il ciclo del motore si progetta con le due fasi esplicite.
- Le interfacce interne del motore non devono permettere chiamate al kernel nella fase di calcolo: niente `std::sync::Mutex` né canali che possano bloccare; strutture senza lock; memoria preallocata e bloccata con `mlockall`.
- **I plugin sono il punto debole**: un plugin che chiama il kernel durante `process()` viola il principio. Serve una misura per plugin e un modo di segnalarlo all'utente; la politica si decide dopo le misure.

### 6. Misure e criteri di accettazione

| Misura | Obiettivo |
| --- | --- |
| Syscall per ciclo sui thread del motore, divise tra calcolo e attesa | Zero nel calcolo; al massimo due nell'attesa |
| Ingressi nel kernel non volontari sui core live (interrupt, anche tra core), per tipo e per fase del ciclo | Zero nel calcolo, con le eccezioni documentate una per una |
| Tempo perso nel calcolo **senza** ingresso nel kernel | Misurato e con un limite dichiarato; non promesso a zero |

_La terza misura è un'integrazione dell'architetto, accolta._ Il caso peggiore delle 24 ore della campagna non è né una syscall né un interrupt: sono pause di circa 18 µs che vengono da sotto il sistema operativo (punto aperto A-43). Le prime due misure non le vedrebbero. Il motore le rileva da sé, guardando i buchi nel contatore del processore durante il calcolo, come fa già `dawos-osnoise --user`.

Le tre misure entrano in `dawos-osnoise` o in uno strumento affiancato, e diventano un controllo ripetibile.

**Lo strumento per le syscall** era lasciato alla scelta dell'architetto. Propongo due strumenti, uno per contare e uno per attribuire _(da verificare entrambi sul 285K, Fase 1)_:

- **per contare**: un contatore dell'evento `raw_syscalls:sys_enter` legato al singolo thread, letto alla fine della misura. Non campiona e non scrive tracce.
- **per attribuire**: il meccanismo _Syscall User Dispatch_ del kernel (`prctl(PR_SET_SYSCALL_USER_DISPATCH)`). Il thread tiene un byte in memoria: quando vale «blocca», ogni syscall fatta da codice fuori da una regione ammessa produce un segnale invece di essere eseguita. Il motore mette il byte a «blocca» entrando nel calcolo e a «permetti» uscendone, senza syscall; il gestore del segnale annota quale plugin ha chiamato il kernel, esegue la chiamata al suo posto e prosegue. Finché nessuno chiama il kernel non costa nulla. Il supporto è nel kernel dell'immagine _(verificato nei sorgenti e nella configurazione della 6.18.64: `GENERIC_SYSCALL`)_.

## Alternative scartate

Il confronto completo sta nel documento di progetto. Qui ciò che la nota del 19 novembre 2026 esclude in modo esplicito, e ciò che le integrazioni hanno scartato.

| Alternativa | Perché no |
| --- | --- |
| Un port di coreboot per il 285K | Non esiste un port per Z890; FSP, CSME, microcodice e ACM resterebbero comunque obbligatori; Boot Guard è forse attivo. Al suo posto, la verifica del firmware a ogni avvio (A-48). |
| Riscrivere il kernel: microkernel, co-kernel EVL, hypervisor di partizionamento | Scartati; il terzo perché incompatibile con i plugin Linux dell'[ADR 0020](0020-plugin-linux-di-terze-parti.md). È ammesso un insieme piccolo di patch, a condizioni (A-49). |
| Riscrivere tutti i driver | Solo quello della scheda audio è nel percorso real-time, ed è l'unico candidato; gli altri si tengono lontani dai core live con la configurazione (A-50). |
| Zero syscall in assoluto, con il motore in attesa attiva | I thread del kernel legati al core non girerebbero mai (K-04), e il core non riposerebbe mai. |
| Lasciare che un ciclo sfori e affidarsi al solo sorvegliante esterno | La protezione arriverebbe dopo più cicli senza attesa, e intervenire dall'esterno interrompe il core live. Resta come seconda linea. |
| Promettere «nessuna interruzione» senza la terza misura | Le pause viste nella campagna non sono interruzioni: una promessa che non le conta non descrive ciò che il motore vedrà. |

## Conseguenze

- **Il thread principale non può svegliare i lavoratori**: è la conseguenza più vincolante per chi progetta il motore, e va decisa prima di scrivere lo scheduler del grafo.
- **Chi sveglia il motore diventa una scelta di progetto.** La «chiamata bloccante» può aspettare un timer del core live stesso, come nella campagna, oppure l'interrupt della scheda audio, che arriva su un core del dominio io e risveglia il core live da lontano. Nel primo caso il motore segue l'orologio della scheda leggendone il puntatore in memoria; nel secondo il risveglio attraversa due core. Va deciso insieme alla misura di A-50.
- **Il ritardo di risveglio misurato nella Fase 0 è il tempo che separa l'attesa dal calcolo**: 2 µs in media, circa 21 µs al massimo in 24 ore, con il core a riposo in C1 ([ADR 0009](0009-riposo-dei-core-live.md)).
- **La soglia di utilizzo del dominio live, 60 %** ([ADR 0011](0011-r04-carico-sintetico.md)), lascia in condizioni normali almeno il 40 % del periodo all'attesa: il bilancio di calcolo del punto 4 conta solo quando la sessione è oltre la soglia.
- **Memoria**: `mlockall` non basta da solo. Vanno verificati sull'immagine anche lo spostamento delle pagine bloccate da parte del compattatore (`vm.compact_unevictable_allowed`) e le pagine grandi trasparenti _(da verificare, Fase 1; le letture del 19 novembre 2026 ne registrano lo stato)_.
- **Leggere l'ora** con `clock_gettime` non è una syscall su questa architettura (passa dalla vDSO): è ammesso nel calcolo.
- **Il blocco del kernel** (decisione D-24, rinviata) impedirebbe la lettura dei registri del processore dallo spazio utente, su cui si basa la verifica del firmware di A-48: le due cose vanno decise insieme.

## Quando si riapre

- Se la misura mostra che il margine del bilancio di calcolo deve essere così grande da togliere capacità utile ai core live.
- Se il rinvio degli interrupt tra core (A-49) non si adotta e nel calcolo restano interrupt che non si riescono a spostare con la configurazione.
- Se nessuna configurazione del driver audio rispetta insieme la latenza richiesta e questo principio, e il driver proprio (A-50) non si scrive.
- Se i plugin che chiamano il kernel durante `process()` risultano troppi per trattarli come eccezioni.

## Precisazioni del 21 novembre 2026

Decise dall'utente all'apertura della Fase 1, sul [progetto P1.1](../fasi/fase-1/P1.1-progetto-ciclo-e-k04.md). Chiudono la scelta che le conseguenze qui sopra lasciavano aperta («chi sveglia il motore») e sostituiscono la domanda D-11 del documento 0.

**Chi sveglia i thread live (D-26).** La scheda audio, per tutti: ogni thread live fa la propria attesa sul dispositivo, e il kernel li sveglia tutti quando il periodo è pronto. Nessun thread ne sveglia un altro, e non c'è un secondo orologio. Se la dispersione dei risvegli tra i thread, misurata nel passo P1.6, risulta troppa, il ripiego è un timer proprio per i worker, con attesa attiva fino all'inizio del ciclo. Scartato: il primo thread che sveglia gli altri, perché è una syscall nel calcolo.

_Come, dal 23 novembre 2026 (passo P1.6b, punto aperto A-56)_: il thread che tiene la scheda dorme sul flusso di ingresso; ognuno degli altri dorme su un timer proprio del kernel legato a quel flusso (`/dev/snd/timer`), che scatta nello stesso punto in cui il primo viene svegliato, e prende il numero del ciclo da ciò che il primo pubblica in memoria. Due syscall nell'attesa per chi segue (`poll()` e `read()` sul timer). La `poll()` di tutti sullo stesso flusso, provata per prima, non regge: un thread svegliato ricontrolla se il periodo c'è ancora, e se il primo l'ha già letto resta a dormire.

**La regola sulle chiamate di sistema (D-11).**

| Fase | Chiamate ammesse su un thread live |
| --- | --- |
| Calcolo | Nessuna |
| Attesa | La chiamata bloccante; sul primo thread, in più, la notifica del puntatore di uscita |
| Fuori dal ciclo (avvio, cambio di grafo che aggiunge o toglie un worker, arresto) | `futex`, per parcheggiare e riprendere un worker |

La fa rispettare il meccanismo _Syscall User Dispatch_ del punto 6, acceso nel calcolo e spento nell'attesa; il filtro seccomp proposto in origine non distingue le fasi e non si usa.

**Il bilancio di calcolo** (punto 4) parte da un margine di 150 µs, da sostituire con il valore misurato. La protezione per il thread che non ritorna affatto è nell'[ADR 0023](0023-protezione-dal-thread-che-non-cede.md).
