// scdesk_journal — ACSIL fill logger (phase 4).
// Remote-build inside Sierra Chart. Writes append-only NDJSON:
//   {DataFolder}/scdesk/fills.ndjson
// Desktop journal prefers these files when present; SCS trades_*.ndjson still imports.

#include "sierrachart.h"

SCDLLName("scdesk journal fills")

static int LastFillIndex = 0;

SCSFExport scsf_ScdeskJournal(SCStudyInterfaceRef sc)
{
    if (sc.SetDefaults)
    {
        sc.GraphName = "scdesk journal fills";
        sc.StudyDescription = "Append order fills to Data/scdesk/fills.ndjson";
        sc.AutoLoop = 0;
        sc.UpdateAlways = 1;
        sc.GraphRegion = 0;
        return;
    }

    int n = sc.GetOrderFillArraySize();
    if (n <= LastFillIndex)
        return;

    SCString path = sc.DataFilesFolder();
    path += "scdesk";
    sc.CreateFolder(path);
    path += "/fills.ndjson";

    int file = sc.OpenFile(path, n_ACSIL::FILE_MODE_OPEN_EXISTING_FOR_SEQUENTIAL_WRITING);
    if (file <= 0)
        file = sc.OpenFile(path, n_ACSIL::FILE_MODE_CREATE_AND_OPEN_FOR_SEQUENTIAL_WRITING);
    if (file <= 0)
        return;

    for (int i = LastFillIndex; i < n; ++i)
    {
        s_SCOrderFillData fill;
        if (!sc.GetOrderFillEntry(i, fill))
            continue;
        SCString line;
        line.Format(
            "{\"source\":\"acsil\",\"symbol\":\"%s\",\"account\":\"%s\",\"side\":%d,\"qty\":%g,\"price\":%g,\"posQty\":%g,\"ts\":\"%s\"}\n",
            fill.Symbol.GetChars(),
            fill.ServiceAccount.GetChars(),
            (int)fill.BuySell,
            fill.Quantity,
            fill.FillPrice,
            fill.TradePositionQuantity,
            sc.DateTimeToString(fill.FillDateTime, FLAG_DT_YEAR | FLAG_DT_MONTH | FLAG_DT_DAY | FLAG_DT_HOUR | FLAG_DT_MINUTE | FLAG_DT_SECOND).GetChars());
        unsigned int written = 0;
        sc.WriteFile(file, line.GetChars(), line.GetLength(), &written);
    }
    sc.CloseFile(file);
    LastFillIndex = n;
}
