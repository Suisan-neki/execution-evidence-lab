"""一つのプロセスの観測を、試行識別子と呼び出し元の場所へ対応づける。"""
import inspect
import json
import os
from pathlib import Path
class Observer:
    def __init__(self):
        for key in ('EEL_RUN_ID','EEL_RECORD_ID','EEL_ATTEMPT_ID','EEL_OBSERVATIONS','EEL_SOURCE_ROOT'):
            if not os.environ.get(key):
                raise ValueError('mugiurushiから試行を起動してください。')
        self.sequence = 0
        self.correlation = {'run_id':os.environ['EEL_RUN_ID'],'record_id':os.environ['EEL_RECORD_ID'],'attempt_id':os.environ['EEL_ATTEMPT_ID']}
    def emit(self, assertion, subject, outcome, detail):
        if not all(isinstance(value,str) and value.strip() for value in (assertion,subject,detail)) or outcome not in ('pass','fail'):
            raise ValueError('観測名・対象・結果・説明を確認してください。')
        frame = inspect.currentframe().f_back
        # プロセスが申告する場所。独立した実行経路の計測ではない。
        path = Path(frame.f_code.co_filename).resolve().relative_to(Path(os.environ['EEL_SOURCE_ROOT']).resolve()).as_posix()
        self.sequence += 1
        event = {'schema_version':1,'sequence':self.sequence,'correlation':self.correlation,'assertion':assertion,'subject':subject,'outcome':outcome,'detail':detail,'path':path,'line':frame.f_lineno}
        with open(os.environ['EEL_OBSERVATIONS'],'a') as output:
            output.write(json.dumps(event,ensure_ascii=False)+'\n')
        return event
