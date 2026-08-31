// scdesk_journal — ACSIL fill logger + halt/replay sidecar reader.
// Remote-build inside Sierra Chart.
// Writes: {DataFolder}/scdesk/fills.ndjson
// Reads:  {DataFolder}/scdesk/tm_halt.json  (journal rules)
//         {DataFolder}/scdesk/replay.json   (journal replay command)

#include "sierrachart.h"
#include <cstring>

SCDLLName("scdesk journal fills")

static bool JsonFlagTrue(const char* buf, const char* key)
{
    const char* p = strstr(buf, key);
    if (!p)
        return false;
    p += strlen(key);
    while (*p == ' ' || *p == '\t')
        ++p;
    return strncmp(p, "true", 4) == 0;
}

static int ReadSidecar(SCStudyInterfaceRef sc, const char* name, char* buf, unsigned int cap)
{
    SCString path = sc.DataFilesFolder();
    path += "scdesk/";
    path += name;
    int file = sc.OpenFile(path, n_ACSIL::FILE_MODE_OPEN_EXISTING_FOR_SEQUENTIAL_READING);
    if (file <= 0)
        return 0;
    unsigned int nread = 0;
    sc.ReadFile(file, buf, cap - 1, &nread);
    sc.CloseFile(file);
    if (nread >= cap)
        nread = cap - 1;
    buf[nread] = 0;
    return (int)nread;
}

SCSFExport scsf_ScdeskJournal(SCStudyInterfaceRef sc)
{
    int& lastFill = sc.GetPersistentInt(1);
    int& haltOn = sc.GetPersistentInt(2);
    int& replaySeen = sc.GetPersistentInt(3);

    if (sc.SetDefaults)
    {
        sc.GraphName = "scdesk journal fills";
        sc.StudyDescription = "Append fills to Data/scdesk/fills.ndjson; honor tm_halt.json / replay.json";
        sc.AutoLoop = 0;
        sc.UpdateAlways = 1;
        sc.GraphRegion = 0;
        return;
    }

    SCString folder = sc.DataFilesFolder();
    folder += "scdesk";
    sc.CreateFolder(folder);

    char sidecar[2048];
    if (ReadSidecar(sc, "tm_halt.json", sidecar, sizeof(sidecar)))
    {
        int now = JsonFlagTrue(sidecar, "\"halt\":") ? 1 : 0;
        if (now != haltOn)
        {
            haltOn = now;
            if (now)
            {
                sc.AddMessageToLog("scdesk: trading halt (journal rules)", 1);
                sc.SetAlert(1, SCString("scdesk halt"));
            }
            else
                sc.AddMessageToLog("scdesk: halt cleared", 0);
        }
    }

    if (ReadSidecar(sc, "replay.json", sidecar, sizeof(sidecar)))
    {
        unsigned int h = 2166136261u;
        for (int i = 0; sidecar[i]; ++i)
            h = (h ^ (unsigned char)sidecar[i]) * 16777619u;
        if ((int)h != replaySeen)
        {
            replaySeen = (int)h;
            sc.AddMessageToLog("scdesk: replay.json updated — start Sierra replay at the datetime in Data/scdesk/replay.json", 0);
        }
    }

    int n = sc.GetOrderFillArraySize();
    if (n <= lastFill)
        return;

    SCString path = folder;
    path += "/fills.ndjson";

    int file = sc.OpenFile(path, n_ACSIL::FILE_MODE_OPEN_EXISTING_FOR_SEQUENTIAL_WRITING);
    if (file <= 0)
        file = sc.OpenFile(path, n_ACSIL::FILE_MODE_CREATE_AND_OPEN_FOR_SEQUENTIAL_WRITING);
    if (file <= 0)
        return;

    for (int i = lastFill; i < n; ++i)
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
    lastFill = n;
}
