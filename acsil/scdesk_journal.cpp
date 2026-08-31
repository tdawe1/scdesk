// scdesk_journal — ACSIL fill logger + halt flatten + replay sidecar.
// Remote-build inside Sierra Chart.
// Writes: {DataFolder}/scdesk/fills.ndjson
// Reads:  {DataFolder}/scdesk/tm_halt.json  (journal rules / prop floor)
//         {DataFolder}/scdesk/replay.json   (journal replay command)

#include "sierrachart.h"
#include <cstring>
#include <cstdio>

SCDLLName("scdesk journal fills")

static void JsonString(const char* buf, const char* key, char* out, int cap)
{
    out[0] = 0;
    const char* p = strstr(buf, key);
    if (!p)
        return;
    p += strlen(key);
    while (*p == ' ' || *p == '\t')
        ++p;
    if (*p == '"')
        ++p;
    int n = 0;
    while (*p && *p != '"' && n + 1 < cap)
        out[n++] = *p++;
    out[n] = 0;
}

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

static int HasWorkingOrders(SCStudyInterfaceRef sc)
{
    int n = 0;
    s_SCTradeOrder order;
    int index = 0;
    while (sc.GetOrderByIndex(index, order))
    {
        ++index;
        if (IsWorkingOrderStatus(order.OrderStatusCode))
            ++n;
    }
    return n;
}

SCSFExport scsf_ScdeskJournal(SCStudyInterfaceRef sc)
{
    int& lastFill = sc.GetPersistentInt(1);
    int& haltOn = sc.GetPersistentInt(2);
    int& replaySeen = sc.GetPersistentInt(3);
    int& flattenTick = sc.GetPersistentInt(4);

    if (sc.SetDefaults)
    {
        sc.GraphName = "scdesk journal fills";
        sc.StudyDescription =
            "Append fills to Data/scdesk/fills.ndjson; flatten/cancel when tm_halt.json halt=true";
        sc.AutoLoop = 0;
        sc.UpdateAlways = 1;
        sc.GraphRegion = 0;
        sc.SupportTrading = true;
        sc.AllowMultipleEntriesInSameDirection = true;
        sc.MaximumPositionAllowed = 100000;
        sc.SupportReversals = true;
        sc.AllowOppositeEntryWithOpposingPositionOrOrders = true;
        sc.CancelAllWorkingOrdersOnExit = false;
        sc.AllowEntryWithWorkingOrders = true;

        sc.Input[0].Name = "Flatten on halt";
        sc.Input[0].SetYesNo(true);
        sc.Input[1].Name = "Send flatten to trade service";
        sc.Input[1].SetYesNo(true);
        sc.Input[2].Name = "Flatten entire trade account";
        sc.Input[2].SetYesNo(true);
        sc.Input[3].Name = "Start chart replay from replay.json";
        sc.Input[3].SetYesNo(true);
        sc.Input[4].Name = "Replay speed";
        sc.Input[4].SetFloat(1.0f);
        sc.ReceiveNotificationsForChangesToOrdersPositionsForAnySymbol = 1;
        return;
    }

    sc.SendOrdersToTradeService = sc.Input[1].GetYesNo();

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
            flattenTick = 0;
            if (now)
            {
                sc.AddMessageToLog("scdesk: trading halt (journal rules)", 1);
                sc.SetAlert(1, SCString("scdesk halt"));
            }
            else
                sc.AddMessageToLog("scdesk: halt cleared", 0);
        }
    }

    if (haltOn && sc.Input[0].GetYesNo())
    {
        ++flattenTick;
        s_SCPositionData pos;
        sc.GetTradePosition(pos);
        int working = HasWorkingOrders(sc);
        int anyPos = pos.PositionQuantity != 0 || working > 0;
        for (int i = 0; i < 64 && !anyPos; ++i)
        {
            s_SCPositionData p;
            if (!sc.GetTradePositionByIndex(p, i))
                break;
            if (p.PositionQuantity != 0 || p.WorkingOrdersExist)
                anyPos = 1;
        }
        if (anyPos && (flattenTick == 1 || flattenTick % 30 == 0))
        {
            int okChart = sc.FlattenAndCancelAllOrders();
            int okAcct = 1;
            if (sc.Input[2].GetYesNo())
            {
                okAcct = sc.FlattenPositionsAndCancelOrdersForTradeAccount(sc.SelectedTradeAccount);
                for (int i = 0; i < 64; ++i)
                {
                    s_SCPositionData p;
                    if (!sc.GetTradePositionByIndex(p, i))
                        break;
                    if (p.PositionQuantity == 0 && !p.WorkingOrdersExist)
                        continue;
                    sc.FlattenAndCancelAllOrdersForSymbolAndNonSimTradeAccount(p.Symbol, p.TradeAccount);
                }
            }
            SCString msg;
            msg.Format(
                "scdesk: flatten chart=%d account=%d pos=%g working=%d",
                okChart,
                okAcct,
                pos.PositionQuantity,
                working);
            sc.AddMessageToLog(msg, 1);
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
            if (sc.Input[3].GetYesNo())
            {
                char dtbuf[64];
                char symbuf[64];
                JsonString(sidecar, "\"datetime\":", dtbuf, sizeof(dtbuf));
                JsonString(sidecar, "\"symbol\":", symbuf, sizeof(symbuf));
                SCDateTime start;
                int y = 0, mo = 0, d = 0, hh = 0, mm = 0, ss = 0;
                if (sscanf(dtbuf, "%d-%d-%dT%d:%d:%d", &y, &mo, &d, &hh, &mm, &ss) == 6
                    || sscanf(dtbuf, "%d-%d-%d %d:%d:%d", &y, &mo, &d, &hh, &mm, &ss) == 6)
                    start.SetDateTimeYMDHMS(y, mo, d, hh, mm, ss);
                int chart = sc.ChartNumber;
                if (symbuf[0])
                {
                    for (int c = 1; c <= 200; ++c)
                    {
                        SCString cs = sc.GetChartSymbol(c);
                        if (cs.GetLength() && strstr(cs.GetChars(), symbuf))
                        {
                            chart = c;
                            break;
                        }
                    }
                }
                n_ACSIL::s_ChartReplayParameters rp;
                rp.ChartNumber = chart;
                rp.ReplaySpeed = sc.Input[4].GetFloat();
                rp.StartDateTime = start;
                rp.ChartsToReplay = n_ACSIL::CHARTS_TO_REPLAY_ALL_CHARTS_IN_CHARTBOOK;
                rp.ClearExistingTradeSimulationDataForSymbolAndTradeAccount = 0;
                int ok = sc.StartChartReplayNew(rp);
                if (!ok)
                    ok = sc.StartChartReplay(chart, sc.Input[4].GetFloat(), start);
                SCString msg;
                msg.Format("scdesk: StartChartReplay chart=%d result=%d %s", chart, ok, dtbuf);
                sc.AddMessageToLog(msg, 0);
            }
            else
                sc.AddMessageToLog(
                    "scdesk: replay.json updated — enable 'Start chart replay' or start Sierra replay manually",
                    0);
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
            sc.DateTimeToString(fill.FillDateTime, FLAG_DT_YEAR | FLAG_DT_MONTH | FLAG_DT_DAY | FLAG_DT_HOUR | FLAG_DT_MINUTE | FLAG_DT_SECOND)
                .GetChars());
        unsigned int written = 0;
        sc.WriteFile(file, line.GetChars(), line.GetLength(), &written);
    }
    sc.CloseFile(file);
    lastFill = n;
}
