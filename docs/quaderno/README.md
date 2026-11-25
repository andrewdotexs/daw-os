# Il quaderno di dawOS

Questo quaderno spiega **che cosa stiamo costruendo, come lo proviamo e dove abbiamo sbagliato**. È scritto per essere letto da capo, da chi sa programmare ma non ha mai messo le mani su un sistema audio in tempo reale.

Non sostituisce le guide dei passi (`docs/fasi/`), che dicono che cosa fare e che cosa è stato misurato, né le decisioni (`docs/adr/`). Le racconta: mette in fila i concetti, dice che cosa vogliono dire i numeri che le prove stampano, e tiene il conto degli errori.

## Come è fatto

| Capitolo | Che cosa c'è | Quando serve |
| --- | --- | --- |
| [1. Il quadro](01-il-quadro.md) | Che cos'è dawOS, perché è fatto così, i numeri che contano | Per cominciare |
| [2. I concetti](02-i-concetti.md) | L'audio digitale, il tempo reale, la scheda, il motore, il grafo, il calcolo: dal basso verso l'alto | Quando un termine non è chiaro |
| [3. Le prove e come leggerle](03-le-prove.md) | Ogni strumento di misura, ogni colonna e ogni riga delle sue uscite | Davanti a un'uscita da interpretare |
| [4. La storia, passo per passo](04-la-storia.md) | Che cosa si è fatto in ogni passo, che cosa si è imparato | Per sapere come ci siamo arrivati |
| [5. Errori e stime sbagliate](05-errori-e-stime.md) | Ciò che avevamo previsto e non era vero, e perché | Per non rifarli |
| [6. Glossario](06-glossario.md) | I termini e le sigle, in ordine alfabetico | Per cercare una parola |

L'ordine consigliato è questo: il quadro, poi i concetti, poi la storia. Il capitolo sulle prove e il glossario si consultano.

## Le convenzioni

- **Un numero è sempre detto con la sua provenienza**: _(verificato)_ se è stato misurato o letto, _(stima)_ se è un conto fatto prima della misura, _(ipotesi)_ se è una spiegazione che i numeri non contraddicono ma non dimostrano.
- **Le sigle** come R-01, K-04, A-68, D-10 sono spiegate nel [glossario](06-glossario.md#le-sigle-del-progetto).
- **Nessun dato privato**: indirizzi, numeri di serie e nomi delle macchine non stanno nel repository. Nei comandi si scrive `INDIRIZZO`.
- I termini inglesi restano in inglese quando sono quelli che si trovano nei manuali e nei messaggi del kernel (_xrun_, _period_, _watchdog_): la prima volta sono spiegati, poi usati.

## Come si aggiorna

Il quaderno si aggiorna **alla chiusura di ogni passo**, insieme alla guida del passo:

1. nel capitolo 4, la voce del passo: che cosa si è fatto, che cosa è venuto fuori;
2. nel capitolo 5, ogni previsione che la misura ha smentito, con ciò che si era previsto, ciò che è stato misurato e il perché, se si sa;
3. nel capitolo 2 i concetti nuovi, nel capitolo 3 gli strumenti nuovi e le colonne nuove, nel glossario i termini nuovi.

Un passo senza errori e senza concetti nuovi ha comunque la sua voce nel capitolo 4.

**Aggiornato a**: passo P1.7e consegnato e in misura sul 285K (25 novembre 2026); P1.7d è l'ultimo passo chiuso.
