# Hardware di riferimento — stato delle conoscenze

- **Scopo**: raccogliere in un solo posto ciò che sappiamo del 285K prima del passo P0.4, distinguendo ciò che è dichiarato, ciò che è verificato e ciò che manca.
- **Aggiornato**: 13 novembre 2026.
- **Regola**: indirizzi di rete, numeri di serie e indirizzi fisici delle schede non entrano nel repository; stanno in `local/`.

## Cosa sappiamo

| Voce | Valore | Fonte |
| --- | --- | --- |
| Processore | Intel Core Ultra 9 285K | Schermata di Windows sul 285K |
| RAM | 64 GB, in funzione a 4800 MT/s | Schermata di Windows |
| Scheda madre | ASUS ROG STRIX Z890-I GAMING WIFI, Mini-ITX, chipset Z890 | Dichiarata |
| Schede grafiche | Intel Graphics integrata e NVIDIA GeForce RTX 5080 da 16 GB | Schermata di Windows |
| Archiviazione | 932 GB visibili, 123 GB usati | Schermata di Windows |
| Sistema attuale | Windows 11, nome `ANDREW-PC` | Schermata di Windows |
| Rete | Cablata, indirizzo IPv4 impostato a mano in Windows | Schermata di Windows |

Dalla scheda tecnica del produttore _(verificato sulla pagina ASUS, non sulla macchina)_:

| Componente | Dato | Conseguenza per dawOS |
| --- | --- | --- |
| Ethernet | Controller Intel da 2,5 Gb | Driver `igc`, con ogni probabilità; modello esatto da leggere con `lspci`. |
| Wi-Fi e Bluetooth | Modulo Wi-Fi 7 2x2 | Resta spento: nessun driver nel kernel. |
| Audio | Nessun codec sulla scheda: tutto passa dal modulo esterno ROG Strix Hive II, collegato via USB | Non lo usiamo; va tenuto scollegato o su un controller USB diverso da quello della Scarlett. |
| Thunderbolt | Due porte Thunderbolt 4 sul pannello posteriore | Sono un controller USB distinto da quello del chipset: è il candidato per isolare la Scarlett (rischio K-07). |
| M.2 | Due alloggiamenti PCIe 5.0; il secondo toglie corsie allo slot della scheda grafica | Spazio per un secondo NVMe, se servisse. |
| Slot | Un solo PCIe x16 | Occupato dalla RTX 5080. |

Non ho trovato nella scheda tecnica: la presenza di un connettore seriale e i dettagli del TPM. Su una Mini-ITX di questa serie un connettore seriale è improbabile _(da verificare sul manuale)_; il TPM sarà quello integrato nel processore (Intel PTT) _(da verificare)_.

## Osservazioni

**La RAM lavora a 4800 MT/s**, cioè al profilo di base e non a un profilo accelerato. Per il tempo reale è la scelta giusta: la lista del firmware nel passo P0.4 chiede proprio un profilo di memoria stabile. Non va cambiato.

**Mini-ITX con 285K e RTX 5080.** Lo spazio ridotto rende calore e rumore delle ventole più critici del previsto: pesa sul rischio K-04 e rende più importanti le misure dell'esperimento E-01 e la verifica di cosa fa la RTX senza driver (K-14).

**Il modulo audio ROG Strix Hive II è un dispositivo USB.** Se resta collegato, il kernel lo vedrà come una seconda scheda audio USB. Nel passo P0.4 va deciso se scollegarlo; nel frattempo conta come dispositivo da tenere lontano dal controller della Scarlett.

## Il disco: deciso

**Decisione del 13 novembre 2026: caso A.** Il disco da 932 GB oggi occupato da Windows 11 è il disco dedicato a dawOS. Il passo P0.4 lo **cancella per intero**, Windows compreso. Il 285K non ha altri dischi da preservare.

Conseguenze:

- **Prima del passo P0.4** va salvato altrove tutto ciò che di quel Windows vuoi tenere (123 GB occupati oggi). Dopo l'installazione non è recuperabile.
- I dati della sezione seguente vanno raccolti **finché Windows è ancora lì**: dopo si possono leggere anche da dawOS, ma avere i due elenchi permette di confrontarli.
- Lo script di scrittura si aspetta **un solo disco NVMe interno**. Se al momento dell'installazione ne trova più d'uno, si ferma: vuol dire che la macchina non è più quella descritta qui.
- Le altre protezioni restano tutte: identificazione per modello e numero di serie, rifiuto del disco da cui si è avviato e dei dischi rimovibili, elenco di ciò che verrà cancellato, conferma digitata con il numero di serie, prova a secco come comportamento predefinito. Che il disco sia «solo» quello di Windows non le rende superflue: servono proprio a evitare di cancellare la chiavetta o un disco aggiunto nel frattempo.
- L'aggiornamento del firmware della scheda madre non richiede Windows: si fa dal firmware stesso, da una chiavetta. Non serve farlo prima di cancellare.

Le alternative scartate erano un secondo NVMe (caso B) e il mantenimento di Windows sul 285K (caso C), che avrebbe riaperto la decisione D-01.

## Dati da raccogliere finché c'è Windows

Sul 285K, in PowerShell aperto come amministratore. Ogni comando va copiato da solo; incollami le uscite, togliendo se vuoi i numeri di serie (quelli servono a te, in `local/`).

Dischi presenti, con modello e numero di serie:

```powershell
Get-PhysicalDisk | Format-List FriendlyName, SerialNumber, MediaType, BusType, Size
```

Scheda madre e versione del firmware:

```powershell
Get-CimInstance Win32_BaseBoard | Format-List Manufacturer, Product, Version
```

```powershell
Get-CimInstance Win32_BIOS | Format-List SMBIOSBIOSVersion, ReleaseDate
```

Stato del TPM e di Secure Boot:

```powershell
Get-Tpm | Format-List TpmPresent, TpmReady, ManufacturerIdTxt, ManufacturerVersion
```

```powershell
Confirm-SecureBootUEFI
```

Schede di rete:

```powershell
Get-NetAdapter | Format-Table Name, InterfaceDescription, LinkSpeed, Status
```

Server DNS configurati, che completano l'indirizzo impostato a mano:

```powershell
Get-DnsClientServerAddress -AddressFamily IPv4 | Format-Table InterfaceAlias, ServerAddresses
```

Controller USB:

```powershell
Get-PnpDevice -PresentOnly | Where-Object { $_.FriendlyName -match 'xHCI|Host Controller' } | Format-Table FriendlyName, Status
```

Un'avvertenza sul numero di serie: per i dischi NVMe Windows mostra a volte un identificativo diverso da quello che legge Linux. Il valore che fa fede per lo script di scrittura è quello letto da dawOS avviato dalla chiavetta; quello di Windows serve da confronto e per contare i dischi.

## Rete

Il 285K ha oggi un indirizzo impostato a mano in Windows. dawOS usa DHCP, quindi dopo l'installazione riceverebbe dal router un indirizzo qualsiasi, e ogni collegamento SSH andrebbe cercato.

| Alternativa | Vantaggi | Svantaggi |
| --- | --- | --- |
| A. Prenotazione sul router, legata all'indirizzo fisico della scheda di rete | L'immagine resta generica e uguale a quella di QEMU; nessun dato di rete nella costruzione. | Richiede di configurare il router. |
| B. Indirizzo statico nell'immagine, letto da `local/` alla costruzione e applicato solo alla scheda del 285K | Non dipende dal router. | L'immagine `dev` diventa specifica della tua rete; un dato in più da tenere in `local/`. |

**Raccomandazione**: A, se hai accesso al router. Altrimenti B. Si decide nel passo P0.4.
