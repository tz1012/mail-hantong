using System.Globalization;
using System.Text;

internal readonly record struct MailRow(long Id, int AccountId, long UtcSeconds, bool Unread, string Subject, string Sender, string Snippet);

internal static class Program
{
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private static readonly UTF8Encoding OutputUtf8 = new(false);

    public static int Main(string[] args)
    {
        if (args.Length == 1 && args[0] == "--ping")
        {
            Console.WriteLine("PONG");
            return 0;
        }

        if (args.Length != 3 || args[0] != "--serve")
        {
            Console.Error.WriteLine("invalid arguments");
            return 2;
        }

        List<MailRow> rows;
        try
        {
            rows = LoadRows(args[1]);
        }
        catch
        {
            Console.Error.WriteLine("invalid cache");
            return 1;
        }

        try
        {
            RunServer(rows, args[2]);
            return 0;
        }
        catch
        {
            Console.Error.WriteLine("runtime failure");
            return 1;
        }
    }

    private static List<MailRow> LoadRows(string path)
    {
        var text = StrictUtf8.GetString(File.ReadAllBytes(path));
        var rows = new List<MailRow>();
        foreach (var line in text.Split('\n'))
        {
            var row = line.EndsWith('\r') ? line[..^1] : line;
            if (row.Length == 0)
            {
                continue;
            }

            var fields = row.Split('\t');
            if (fields.Length != 7)
            {
                throw new InvalidDataException();
            }

            if (!long.TryParse(fields[0], NumberStyles.None, CultureInfo.InvariantCulture, out var id) || id <= 0 ||
                !TryAccount(fields[1], out var account) ||
                !long.TryParse(fields[2], NumberStyles.Integer, CultureInfo.InvariantCulture, out var utc) ||
                !int.TryParse(fields[3], NumberStyles.None, CultureInfo.InvariantCulture, out var unread) || unread is < 0 or > 1)
            {
                throw new InvalidDataException();
            }

            if (fields[4].Contains('\n') || fields[4].Contains('\r') || fields[5].Contains('\n') || fields[5].Contains('\r') || fields[6].Contains('\n') || fields[6].Contains('\r'))
            {
                throw new InvalidDataException();
            }

            rows.Add(new MailRow(id, account, utc, unread == 1, fields[4], fields[5], fields[6]));
        }

        rows.Sort(static (left, right) =>
        {
            var utc = right.UtcSeconds.CompareTo(left.UtcSeconds);
            return utc != 0 ? utc : right.Id.CompareTo(left.Id);
        });
        return rows;
    }

    private static void RunServer(List<MailRow> rows, string bodyPath)
    {
        using var input = new StreamReader(Console.OpenStandardInput(), StrictUtf8, false, 4096, leaveOpen: false);
        using var output = new StreamWriter(Console.OpenStandardOutput(), OutputUtf8, 4096, leaveOpen: false) { NewLine = "\n", AutoFlush = true };
        output.WriteLine($"READY\t{rows.Count}");
        while (input.ReadLine() is { } line)
        {
            var response = HandleCommand(rows, bodyPath, line);
            output.WriteLine(response);
            if (response == "BYE")
            {
                return;
            }
        }
    }

    private static string HandleCommand(List<MailRow> rows, string bodyPath, string line)
    {
        var fields = line.Split('\t');
        if (fields.Length == 1 && fields[0] == "PING") return "PONG";
        if (fields.Length == 1 && fields[0] == "QUIT") return "BYE";
        if (fields.Length == 1 && fields[0] == "BODY")
        {
            var bytes = File.ReadAllBytes(bodyPath);
            long sum = 0;
            foreach (var value in bytes) sum += value;
            return $"BODY\t{bytes.LongLength}\t{sum}";
        }

        if (fields.Length == 2 && fields[0] == "LIST" && TryAccount(fields[1], out var listAccount))
        {
            var ids = rows.Where(row => row.AccountId == listAccount).Take(100).Select(row => row.Id.ToString(CultureInfo.InvariantCulture));
            var idText = string.Join(',', ids);
            var count = idText.Length == 0 ? 0 : idText.Count(value => value == ',') + 1;
            return $"LIST\t{count}\t{idText}";
        }

        if (fields.Length == 3 && fields[0] == "SEARCH" && TryAccount(fields[1], out var searchAccount) && fields[2].Length > 0)
        {
            long count = 0;
            long sum = 0;
            foreach (var row in rows)
            {
                if (row.AccountId == searchAccount && (row.Subject.Contains(fields[2], StringComparison.Ordinal) || row.Snippet.Contains(fields[2], StringComparison.Ordinal)))
                {
                    count++;
                    sum += row.Id;
                }
            }
            return $"SEARCH\t{count}\t{sum}";
        }

        return "ERR";
    }

    private static bool TryAccount(string value, out int account)
    {
        if (value.Length == 1 && value[0] is >= '1' and <= '5')
        {
            account = value[0] - '0';
            return true;
        }

        account = 0;
        return false;
    }
}
