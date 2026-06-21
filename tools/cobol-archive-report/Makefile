# build and test the archive-report COBOL tool.
# requires GnuCOBOL (cobc) 3.x, which ships the report writer.

COBC ?= cobc
COBCFLAGS ?= -x -free -Wall
BIN := archive-report
SRC := archive-report.cob
SAMPLE := sample.jsonl

.PHONY: all test clean

all: $(BIN)

$(BIN): $(SRC)
	$(COBC) $(COBCFLAGS) $(SRC) -o $(BIN)

# run against the bundled sample and show the report.
test: $(BIN) $(SAMPLE)
	./$(BIN) $(SAMPLE)

clean:
	rm -f $(BIN) $(SAMPLE).gz
