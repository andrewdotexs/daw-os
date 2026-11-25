# ADR 0025 — Il grafo: ogni nodo ha il suo core, deciso alla compilazione

- **Stato**: accettata dall'utente il 25 novembre 2026 (decisione D-10, per la parte che tocca il grafo). Il compilatore è nel passo [P1.7a](../fasi/fase-1/P1.7-il-grafo.md), l'esecuzione nel ciclo in P1.7c; la misura viene con P1.7d
- **Data**: 25 novembre 2026
- **Origine**: domanda D-10 del documento 0 («modello di passaggio tra motore e host»), riproposta in [01-domande-da-d10-a-d21.md](../fasi/fase-1/01-domande-da-d10-a-d21.md); [ADR 0021](0021-core-live-senza-kernel.md); contraddizione C-01 del documento 0

## Contesto

Il motore calcola il grafo su più thread live, uno per core. Quando un nodo è pronto, cioè i suoi ingressi sono calcolati, qualcuno deve eseguirlo. I modi sono due: il nodo ha un thread suo, sempre lo stesso; oppure lo prende il primo thread libero.

La domanda non riguarda solo la Fase 1. Dalla Fase 4 certi nodi saranno plugin ospitati in un altro processo, e l'ADR 0021 vieta di svegliare un altro processo durante il calcolo: l'host del plugin deve quindi essere già sveglio, in attesa attiva, **su un core che conosce in anticipo**.

## Decisione

**Ogni nodo è assegnato a un thread live quando il grafo viene compilato, fuori dal tempo reale, e gira lì a ogni ciclo.**

1. **L'assegnazione è per segmenti.** Una catena di nodi in cui ognuno aspetta solo il precedente, che alimenta solo lui, va per intero su un thread: un segnale non cambia core dove non c'è nulla da guadagnare.
2. **Un thread il cui prossimo nodo aspetta un ingresso da un altro thread resta in attesa attiva**, senza chiamate di sistema, finché l'ingresso è pronto o il bilancio del ciclo è scaduto.
3. **I nodi che toccano la scheda** (ingressi e uscite) stanno sul thread 0, quello che tiene la scheda.
4. **Il compilatore decide con un programma su carta.** Ogni nodo ha un costo atteso; i segmenti si prendono da quello con più lavoro ancora davanti a quello con meno, e ognuno va sul thread dove finirebbe prima, contando quando sono pronti i suoi ingressi, quando è libero il thread e una perdita quando un ingresso arriva da un altro thread.
5. **Non si usano più thread di quanti servono.** Il programma si fa per un thread, per due, fino a tutti, e si tiene il più corto; a parità, quello su meno thread.
6. **Una descrizione può fissare il thread di un nodo** (`thread=N`): serve alle prove, e in Fase 4 ai segmenti esterni.
7. **Il grafo non ha vie di ritorno**: un cerchio tra nodi è un errore. Una linea di ritardo non lo apre, per ora.

## Come si aspetta (dal passo P1.7c)

Ogni nodo porta il numero dell'ultimo ciclo per cui è stato eseguito. Un thread, prima di un nodo, legge i numeri dei nodi che quel nodo aspetta: se sono tutti al ciclo corrente lo esegue; se qualcuno manca gira a vuoto rileggendoli, fino al bilancio del ciclo ([ADR 0023](0023-protezione-dal-thread-che-non-cede.md)); se qualcuno è già a un ciclo successivo il thread è indietro, e chiude il ciclo. Un nodo i cui ingressi non arrivano in tempo è un ciclo tagliato.

I buffer e lo stato dei nodi stanno dietro lucchetti che vengono solo tentati, mai attesi: nessun `unsafe` fuori da `dawos-rt::sys`, e un errore nell'ordine diventa un contatore (`graph_refused`) invece di una corsa sui dati. Il costo, un paio di operazioni atomiche per buffer, lo misura P1.7d.

## Perché

- **È ciò che la Fase 4 richiede**: con una coda comune l'host di un plugin non saprebbe su quale core aspettare.
- **Il ciclo è lo stesso a ogni periodo.** Gli stessi nodi, nello stesso ordine, sugli stessi core: i tempi si ripetono, e una misura fatta oggi vale domani.
- **I dati di un nodo restano nella cache del suo core.**
- **È il modello più semplice da provare**: l'ordine in cui un thread esegue i suoi nodi è fisso, e l'unica interazione tra thread è l'attesa di un ingresso.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Coda comune dei nodi pronti: li prende il primo thread libero | Sfrutta meglio i core con grafi sbilanciati, ma lo stesso nodo cambia core da un ciclo all'altro: tempi meno ripetibili, cache fredde, e l'host di un plugin non sa dove aspettare. |
| Assegnazione statica con furto di lavoro come riserva | Più codice e più casi da provare. Si può aggiungere se le misure di accelerazione di P1.7d lo chiedono. |
| Assegnare nodo per nodo invece che per segmenti | Provata scrivendo il compilatore: su sedici tracce in parallelo sparpaglia i nodi di una traccia su tutti i thread, con decine di passaggi tra core per un guadagno sulla carta del 3 %. |
| Usare sempre tutti i thread | Un mixer di otto ingressi finiva con un guadagno spostato su un altro core e un ciclo più lungo che su un core solo. |

## Conseguenze

- **L'assegnazione vale quanto i costi da cui parte.** Oggi sono stime, tranne il carico di R-04 che è calibrato per costare ciò che dichiara. La perdita per un ingresso da un altro thread, 1 µs, è una stima mia: P1.7d la misura.
- **Un grafo sbilanciato lascia core fermi.** Sedici tracce uguali si dividono bene; una traccia sola che pesa quanto tutte le altre fa il percorso critico, e gli altri core aspettano.
- **Attesa attiva**: un thread che aspetta un ingresso tiene sveglio il suo core (rischio K-23). Con l'assegnazione per segmenti le attese sono poche; quante e quanto lunghe si misura in P1.7d.
- **Cambiare grafo vuol dire ricompilare**, anche per spostare un solo nodo. È fuori dal tempo reale, e lo scambio è il sottopasso P1.7e.
- **Il programma su carta è un'euristica**, non l'ottimo: l'ottimo è un problema difficile, e qui conta che il risultato sia ripetibile e spiegabile.

## Quando si riapre

- Se in P1.7d l'accelerazione misurata su quattro core è molto sotto quella prevista sulla carta.
- Se le attese attive tra thread scaldano i core live o tolgono il riposo (K-23).
- Se i lucchetti tentati costano troppo rispetto ai nodi leggeri: allora un tipo «buffer del grafo» dentro `dawos-rt`, con il suo `unsafe` documentato.
- In Fase 4, quando esistono i segmenti esterni: il passaggio sincrono sullo stesso core va progettato lì.
- Se serve un ritorno nel grafo (un effetto in retroazione): allora una linea di ritardo di almeno un periodo deve poter aprire il cerchio.
