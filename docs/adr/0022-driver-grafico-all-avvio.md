# ADR 0022 — Driver grafico caricato a ogni avvio

- **Stato**: accettata dall'utente il 20 novembre 2026, per la variante `dev` e il profilo `285k`; da confermare con la misura lunga della parte 3 di [P1.0](../fasi/fase-1/P1.0-raccordo.md). Cambia la decisione presa nella Fase 0 sul punto A-02 («il driver non si carica all'avvio»)
- **Data**: 20 novembre 2026
- **Origine**: sotto-passo [P1.0](../fasi/fase-1/P1.0-raccordo.md), parte 1; punti aperti A-43 e A-02; esperimento E-04 (P0.6c); rischio K-16

## Contesto

Nella Fase 0 il driver della grafica integrata, `xe`, era un modulo che nulla caricava: la console restava sull'immagine preparata dal firmware. La scelta aveva due motivi. `xe` è l'unico driver che si compila per questa GPU su un kernel real-time, ed è sperimentale per questo processore in Linux 6.18: si lega al dispositivo solo con l'opzione `force_probe`. E l'esperimento E-04 aveva mostrato che con lo schermo fermo il driver non porta rumore, ma non che servisse a qualcosa.

La campagna di misura [P0.7](../fasi/fase-0/P0.7-campagna-di-misura.md) è stata fatta così: schermo collegato, acceso dal firmware, nessun driver. In 24 ore ha trovato sei risvegli sopra i 20 µs, fino a 20,7 µs, senza una causa nel sistema operativo (punto aperto A-43).

Il 20 novembre 2026, nella parte 1 di P1.0, la causa pratica è stata isolata _(verificato sul 285K, stessa immagine)_:

| Condizione | Massimo in un quarto d'ora | Risvegli da 10 µs in su |
| --- | --- | --- |
| Nessuno schermo all'accensione, miscela di carico intera | 7,14 µs | 0 |
| Schermo acceso dal firmware, nessun driver, miscela intera | 12,36 e 19,08 µs (due avvii) | 36 e 63 |
| Lo stesso, senza il disco | 12,02 µs | 3 |
| Lo stesso, senza il lavoro sulla memoria | 6,69 µs | 0 |
| Schermo acceso dal firmware, **driver `xe` caricato**, miscela intera | 7,16 µs | 0 |
| Lo stesso, sull'immagine che carica il driver all'avvio (aggiunto dopo la verifica della parte 2 di P1.0) | 6,54 µs | 0 |

Con uno schermo che il firmware ha acceso e che nessun driver prende in carico, sotto un carico che muove molta memoria i core live perdono 10–20 µs alla volta. Con il driver caricato non succede.

## Decisione

**Nel profilo `285k` il driver `xe` si carica a ogni avvio, e deve aver preso la GPU perché il profilo risulti applicato.**

1. **Resta un modulo.** Lo carica `dawos-tune`, per nome e con l'opzione `force_probe` del dispositivo che trova, come primo passo della regolazione: prima di spostare gli interrupt, così quello del driver nasce già sui core del dominio io.
2. **Lo chiede il profilo**, con la riga `graphics_driver=xe`. Un profilo senza quella riga non carica nulla; il profilo `generic` non ha domini e non carica nulla.
3. **Se il driver non prende la GPU il profilo è `degraded`**, come per ogni altra parte della regolazione che non si riesce ad applicare: l'unità fallisce e l'avvio è marcato come non riuscito ([ADR 0016](0016-valutazione-dell-avvio.md)). Un aggiornamento che rompe il driver torna indietro da solo.
4. **Il kernel non lo carica per conto suo**: `xe` resta escluso dal caricamento automatico.
5. **In una macchina virtuale il passo si salta**, come quelli su frequenza, riposo e potenza.
6. **Il rapporto di avvio dice quale driver tiene lo schermo** (`gpu=xe`, oppure `gpu=none`).
7. **Il piano `pause` della campagna, con lo schermo collegato all'accensione, è la prova di regressione**: un quarto d'ora, nessun risveglio da 10 µs in su. Si ripete a ogni cambio di kernel, di firmware o di driver grafico.

## Perché

- **È la configurazione del prodotto.** dawOS avrà sempre uno schermo e un driver che lo governa. La condizione misurata nella Fase 0, schermo senza driver, non è quella del prodotto ed è la peggiore delle tre.
- **Costa poco e si misura subito**: lo stesso modulo, lo stesso firmware della grafica, già nell'immagine dall'esperimento E-04.
- **Il caricamento di un modulo interrompe tutti i core** (il kernel riscrive il proprio codice): all'avvio non c'è ancora nulla di real-time sui core live. Caricarlo più tardi, quando l'interfaccia della DAW parte, sarebbe il momento sbagliato.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| `xe` dentro il kernel, come KVM | Il driver partirebbe in un altro punto dell'avvio, mai misurato, e i file di firmware della grafica andrebbero incorporati nel kernel. Un avvio senza driver non sarebbe più possibile cambiando profilo. Si può riprendere quando `xe` non sarà più sperimentale per questo processore. |
| Lasciare la console al firmware, come nella Fase 0 | Tiene la coda fino a 20 µs ogni volta che c'è uno schermo: cioè sempre, nel prodotto. |
| Togliere la causa nel firmware | La causa non è nota. L'ipotesi verificabile, il cambio dinamico di frequenza della memoria, è caduta: il driver stesso dice che quella funzione nel firmware è spenta. Per scelta dell'utente non si prova cambiando voci del firmware che la campagna ha certificato. |
| Avviare senza schermo | Non è un uso reale. |
| Segnalare soltanto, senza marcare l'avvio | La differenza è di 12 µs sul caso peggiore: più di quella tra C1 e C1E, che già rende il profilo `degraded`. |

## Conseguenze

- **Il kernel risulta marcato** dall'opzione `force_probe` (il kernel la chiama «pericolosa»): lo si vede nel registro di ogni avvio, e i rapporti delle misure lo riportano.
- **L'immagine non è più «zero moduli caricati»**: con `xe` e ciò da cui dipende i moduli sono undici. I controlli che contavano i moduli vanno letti così.
- **Il rischio K-16 pesa di più**: da «un driver sperimentale che un giorno servirà» a «un driver sperimentale caricato a ogni avvio». Lo copre il punto 3: se si rompe, l'avvio torna all'immagine precedente.
- **Il meccanismo non è noto.** Che cosa, tra ciò che il driver fa e il firmware no, tolga le pause non lo so: il firmware del controllore dello schermo, le soglie della coda di lettura, la priorità delle letture in memoria, la compressione dell'immagine, lo spostamento dell'immagine nella memoria riservata alla grafica. Finché non è noto, la prova di regressione del punto 7 è l'unica garanzia.
- **Il carico grafico vero resta da misurare** (A-23): tutte le misure di oggi sono a schermo fermo.
- **Su un quarto d'ora la coda non c'è; sulle 24 ore non è misurato.** L'obiettivo dei 20 µs (A-43) si chiude con una misura lunga in questa configurazione.

## Quando si riapre

- Se la misura lunga con schermo e driver ritrova risvegli sopra i 20 µs.
- Se `xe` smette di funzionare con un kernel nuovo, o diventa stabile per questo processore e può entrare nel kernel senza `force_probe`.
- Se si trova il meccanismo e risulta governabile dal firmware o dal kernel senza il driver.
- Quando l'interfaccia della DAW esiste e il carico grafico vero si può misurare.
